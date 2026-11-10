use nym_sdk::mixnet::{MixnetClient, MixnetStream};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PINGS: usize = 20;
const PING_SIZE: usize = 64;
const TIMEOUT: Duration = Duration::from_secs(120);

async fn serve(mut stream: MixnetStream) -> anyhow::Result<()> {
    let mut mode = [0u8; 1];
    stream.read_exact(&mut mode).await?;
    match mode[0] {
        b'E' => {
            let mut buf = [0u8; PING_SIZE];
            loop {
                stream.read_exact(&mut buf).await?;
                stream.write_all(&buf).await?;
                stream.flush().await?;
            }
        }
        b'S' => {
            let len = stream.read_u32().await? as usize;
            let mut buf = vec![0u8; len];
            stream.read_exact(&mut buf).await?;
            stream.write_all(b"K").await?;
            stream.flush().await?;
            Ok(())
        }
        m => anyhow::bail!("unknown mode {m}"),
    }
}

fn secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

async fn ping(client: &mut MixnetClient, to: nym_sdk::mixnet::Recipient) -> anyhow::Result<()> {
    let t = Instant::now();
    let mut s = client.open_stream(to, None).await?;
    s.write_all(b"E").await?;
    println!("open_stream: {:.2}s", secs(t.elapsed()));

    let mut rtts = Vec::new();
    let mut buf = [0u8; PING_SIZE];
    for i in 0..PINGS {
        let payload = [i as u8; PING_SIZE];
        let t = Instant::now();
        s.write_all(&payload).await?;
        s.flush().await?;
        match tokio::time::timeout(TIMEOUT, s.read_exact(&mut buf)).await {
            Ok(r) => {
                r?;
                anyhow::ensure!(buf == payload, "echo mismatch on ping {i}");
                let rtt = secs(t.elapsed());
                println!("ping {i}: {rtt:.2}s");
                rtts.push(rtt);
            }
            Err(_) => anyhow::bail!("ping {i} timed out (lost packet stalls the stream)"),
        }
    }
    rtts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "RTT n={} min={:.2}s p50={:.2}s p90={:.2}s max={:.2}s",
        rtts.len(),
        rtts[0],
        percentile(&rtts, 0.5),
        percentile(&rtts, 0.9),
        rtts[rtts.len() - 1]
    );
    Ok(())
}

async fn throughput(client: &mut MixnetClient, to: nym_sdk::mixnet::Recipient, size: usize) -> anyhow::Result<()> {
    let mut s = client.open_stream(to, None).await?;
    let t = Instant::now();
    s.write_all(b"S").await?;
    s.write_u32(size as u32).await?;
    s.write_all(&vec![0xAB; size]).await?;
    s.flush().await?;
    let mut ack = [0u8; 1];
    tokio::time::timeout(Duration::from_secs(600), s.read_exact(&mut ack)).await??;
    let el = secs(t.elapsed());
    println!(
        "throughput {} KiB: {:.2}s => {:.1} KiB/s",
        size / 1024,
        el,
        size as f64 / 1024.0 / el
    );
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let t = Instant::now();
    let mut server = MixnetClient::connect_new().await?;
    let server_addr = *server.nym_address();
    println!("server connected in {:.2}s: {server_addr}", secs(t.elapsed()));

    let t = Instant::now();
    let mut client = MixnetClient::connect_new().await?;
    println!("client connected in {:.2}s: {}", secs(t.elapsed()), client.nym_address());

    let mut listener = server.listener()?;
    tokio::spawn(async move {
        while let Some(stream) = listener.accept().await {
            tokio::spawn(async move {
                if let Err(e) = serve(stream).await {
                    eprintln!("serve: {e}");
                }
            });
        }
    });

    ping(&mut client, server_addr).await?;
    for kib in [64, 1024] {
        throughput(&mut client, server_addr, kib * 1024).await?;
    }

    client.disconnect().await;
    server.disconnect().await;
    Ok(())
}
