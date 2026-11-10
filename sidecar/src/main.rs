mod frame;
mod pump;
mod stats;

use anyhow::Context;
use nym_sdk::mixnet::{MixnetClient, MixnetClientBuilder, MixnetListener, Recipient, StoragePaths};
use stats::Stats;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

struct Config {
    node_ip: Ipv4Addr,
    peers_dir: PathBuf,
    state_dir: PathBuf,
    redirect_port: u16,
    bee_addr: String,
    peer_wait: Duration,
}

impl Config {
    fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        Ok(Self {
            node_ip: std::env::var("NODE_IP").context("NODE_IP not set")?.parse()?,
            peers_dir: var("PEERS_DIR", "/peers").into(),
            state_dir: var("STATE_DIR", "/state").into(),
            redirect_port: var("REDIRECT_PORT", "4001").parse()?,
            bee_addr: var("BEE_ADDR", "127.0.0.1:1634"),
            peer_wait: Duration::from_secs(var("PEER_WAIT_SECS", "120").parse()?),
        })
    }
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

fn log_conn(dir: &str, peer: &str, started: Instant, r: &pump::Report) {
    println!(
        r#"{{"event":"conn","ts":{},"dir":"{dir}","peer":"{peer}","duration_ms":{},"first_response_ms":{},"tcp_to_mix":{},"mix_to_tcp":{},"error":{}}}"#,
        now_ms(),
        started.elapsed().as_millis(),
        r.first_response_ms.map_or("null".into(), |v| v.to_string()),
        r.tcp_to_mix,
        r.mix_to_tcp,
        r.error.as_ref().map_or("null".into(), |e| format!("{e:?}")),
    );
}

fn original_dst(stream: &TcpStream) -> std::io::Result<SocketAddrV4> {
    let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_IP,
            libc::SO_ORIGINAL_DST,
            &mut addr as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(SocketAddrV4::new(
        Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr)),
        u16::from_be(addr.sin_port),
    ))
}

async fn lookup_peer(cfg: &Config, ip: Ipv4Addr) -> anyhow::Result<Recipient> {
    let path = cfg.peers_dir.join(ip.to_string());
    let deadline = Instant::now() + cfg.peer_wait;
    loop {
        match tokio::fs::read_to_string(&path).await {
            Ok(s) => return s.trim().parse().map_err(|e| anyhow::anyhow!("bad nym address for {ip}: {e:?}")),
            Err(_) if Instant::now() < deadline => tokio::time::sleep(Duration::from_millis(500)).await,
            Err(e) => anyhow::bail!("no nym address for {ip}: {e}"),
        }
    }
}

async fn publish_address(cfg: &Config, addr: &Recipient) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&cfg.peers_dir).await?;
    let tmp = cfg.peers_dir.join(format!(".{}.tmp", cfg.node_ip));
    tokio::fs::write(&tmp, addr.to_string()).await?;
    tokio::fs::rename(&tmp, cfg.peers_dir.join(cfg.node_ip.to_string())).await?;
    Ok(())
}

async fn handle_outbound(
    cfg: Arc<Config>,
    client: Arc<Mutex<Option<MixnetClient>>>,
    stats: Arc<Stats>,
    tcp: TcpStream,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let dst = original_dst(&tcp).context("SO_ORIGINAL_DST")?;
    let recipient = lookup_peer(&cfg, *dst.ip()).await?;
    let mix = {
        let mut guard = client.lock().await;
        guard.as_mut().context("mixnet client reconnecting")?.open_stream(recipient, None).await?
    };
    stats.conns_out.fetch_add(1, Relaxed);
    stats.active.fetch_add(1, Relaxed);
    let report = pump::pump(tcp, mix, stats.clone()).await;
    stats.active.fetch_sub(1, Relaxed);
    log_conn("out", &dst.ip().to_string(), started, &report);
    Ok(())
}

async fn handle_inbound(cfg: Arc<Config>, stats: Arc<Stats>, mix: nym_sdk::mixnet::MixnetStream) -> anyhow::Result<()> {
    let started = Instant::now();
    let tcp = TcpStream::connect(&cfg.bee_addr).await.context("connect to local bee")?;
    tcp.set_nodelay(true)?;
    stats.conns_in.fetch_add(1, Relaxed);
    stats.active.fetch_add(1, Relaxed);
    let report = pump::pump(tcp, mix, stats.clone()).await;
    stats.active.fetch_sub(1, Relaxed);
    log_conn("in", "anonymous", started, &report);
    Ok(())
}

async fn connect(cfg: &Config) -> anyhow::Result<(MixnetClient, MixnetListener)> {
    let t = Instant::now();
    let mut client = MixnetClientBuilder::new_with_default_storage(StoragePaths::new_from_dir(&cfg.state_dir)?)
        .await?
        .with_stream_idle_timeout(Duration::from_secs(6 * 3600))
        .build()?
        .connect_to_mixnet()
        .await?;
    let listener = client.listener()?;
    println!(
        r#"{{"event":"connected","ts":{},"connect_ms":{},"node_ip":"{}","nym_address":"{}"}}"#,
        now_ms(),
        t.elapsed().as_millis(),
        cfg.node_ip,
        client.nym_address()
    );
    Ok((client, listener))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .init();
    let cfg = Arc::new(Config::from_env()?);
    let stats = Arc::new(Stats::default());

    let (client, mut listener) = connect(&cfg).await?;
    let address = *client.nym_address();
    let client = Arc::new(Mutex::new(Some(client)));
    let tcp_listener = TcpListener::bind(("0.0.0.0", cfg.redirect_port)).await?;
    publish_address(&cfg, &address).await?;

    let (in_cfg, in_stats, in_client) = (cfg.clone(), stats.clone(), client.clone());
    tokio::spawn(async move {
        loop {
            while let Some(mix) = listener.accept().await {
                let (cfg, stats) = (in_cfg.clone(), in_stats.clone());
                tokio::spawn(async move {
                    if let Err(e) = handle_inbound(cfg, stats.clone(), mix).await {
                        stats.failed.fetch_add(1, Relaxed);
                        eprintln!(r#"{{"event":"error","dir":"in","error":{:?}}}"#, format!("{e:#}"));
                    }
                });
            }
            eprintln!(r#"{{"event":"listener_closed","ts":{}}}"#, now_ms());
            in_client.lock().await.take();
            listener = loop {
                match connect(&in_cfg).await {
                    Ok((new_client, new_listener)) => {
                        *in_client.lock().await = Some(new_client);
                        break new_listener;
                    }
                    Err(e) => {
                        eprintln!(r#"{{"event":"reconnect_failed","error":{:?}}}"#, format!("{e:#}"));
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            };
        }
    });

    let stats_print = stats.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tick.tick().await;
            println!("{}", stats_print.json());
        }
    });

    loop {
        let (tcp, _) = tcp_listener.accept().await?;
        tcp.set_nodelay(true)?;
        let (cfg, client, stats) = (cfg.clone(), client.clone(), stats.clone());
        tokio::spawn(async move {
            if let Err(e) = handle_outbound(cfg, client, stats.clone(), tcp).await {
                stats.failed.fetch_add(1, Relaxed);
                eprintln!(r#"{{"event":"error","dir":"out","error":{:?}}}"#, format!("{e:#}"));
            }
        });
    }
}
