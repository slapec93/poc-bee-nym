use crate::frame::{self, Frame, MAX_PAYLOAD};
use crate::stats::Stats;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

pub struct Report {
    pub tcp_to_mix: u64,
    pub mix_to_tcp: u64,
    pub first_response_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Counters {
    tcp_to_mix: AtomicU64,
    mix_to_tcp: AtomicU64,
    first_response_ms_plus_one: AtomicU64,
}

pub async fn pump<T, M>(tcp: T, mix: M, stats: Arc<Stats>) -> Report
where
    T: AsyncRead + AsyncWrite + Send + 'static,
    M: AsyncRead + AsyncWrite + Send + 'static,
{
    let start = Instant::now();
    let counters = Arc::new(Counters::default());
    let (mut tcp_r, mut tcp_w) = tokio::io::split(tcp);
    let (mix_r, mut mix_w) = tokio::io::split(mix);
    let mut mix_r = BufReader::new(mix_r);

    let (up_stats, up_counters) = (stats.clone(), counters.clone());
    let mut upstream = tokio::spawn(async move {
        let mut buf = vec![0u8; MAX_PAYLOAD];
        loop {
            let n = tcp_r.read(&mut buf).await?;
            if n == 0 {
                return frame::write_fin(&mut mix_w).await;
            }
            frame::write_data(&mut mix_w, &buf[..n]).await?;
            up_counters.tcp_to_mix.fetch_add(n as u64, Relaxed);
            up_stats.bytes_to_mix.fetch_add(n as u64, Relaxed);
        }
    });

    let down_counters = counters.clone();
    let mut downstream = tokio::spawn(async move {
        loop {
            match frame::read(&mut mix_r).await? {
                Frame::Data(payload) => {
                    let elapsed = start.elapsed().as_millis() as u64;
                    let _ = down_counters.first_response_ms_plus_one.compare_exchange(0, elapsed + 1, Relaxed, Relaxed);
                    tcp_w.write_all(&payload).await?;
                    down_counters.mix_to_tcp.fetch_add(payload.len() as u64, Relaxed);
                    stats.bytes_from_mix.fetch_add(payload.len() as u64, Relaxed);
                }
                Frame::Fin => return tcp_w.shutdown().await,
            }
        }
    });

    let mut error = None;
    let (mut up_done, mut down_done) = (false, false);
    while error.is_none() && !(up_done && down_done) {
        tokio::select! {
            r = &mut upstream, if !up_done => {
                up_done = true;
                error = task_error("upstream", r);
            }
            r = &mut downstream, if !down_done => {
                down_done = true;
                error = task_error("downstream", r);
            }
        }
    }
    upstream.abort();
    downstream.abort();

    let first = counters.first_response_ms_plus_one.load(Relaxed);
    Report {
        tcp_to_mix: counters.tcp_to_mix.load(Relaxed),
        mix_to_tcp: counters.mix_to_tcp.load(Relaxed),
        first_response_ms: (first > 0).then(|| first - 1),
        error,
    }
}

fn task_error(side: &str, r: Result<std::io::Result<()>, tokio::task::JoinError>) -> Option<String> {
    match r {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(format!("{side}: {e}")),
        Err(e) => Some(format!("{side} task: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn relay_side<M: AsyncRead + AsyncWrite + Unpin>(mut mix: M, echo_bytes: usize) {
        let mut got = Vec::new();
        loop {
            match frame::read(&mut mix).await.unwrap() {
                Frame::Data(p) => got.extend(p),
                Frame::Fin => break,
            }
        }
        assert_eq!(got.len(), echo_bytes);
        for chunk in got.chunks(MAX_PAYLOAD) {
            frame::write_data(&mut mix, chunk).await.unwrap();
        }
        frame::write_fin(&mut mix).await.unwrap();
    }

    #[tokio::test]
    async fn carries_data_both_ways_and_propagates_close() {
        let (tcp_app, tcp_pumped) = tokio::io::duplex(1 << 16);
        let (mix_pumped, mix_remote) = tokio::io::duplex(1 << 16);
        let size = 200_000;

        let remote = tokio::spawn(relay_side(mix_remote, size));
        let pumping = tokio::spawn(pump(tcp_pumped, mix_pumped, Arc::new(Stats::default())));

        let (mut app_r, mut app_w) = tokio::io::split(tcp_app);
        let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let expected = payload.clone();
        let writer = tokio::spawn(async move {
            app_w.write_all(&payload).await.unwrap();
            app_w.shutdown().await.unwrap();
        });
        let mut echoed = Vec::new();
        app_r.read_to_end(&mut echoed).await.unwrap();

        writer.await.unwrap();
        remote.await.unwrap();
        let report = pumping.await.unwrap();
        assert_eq!(echoed, expected);
        assert_eq!(report.tcp_to_mix, size as u64);
        assert_eq!(report.mix_to_tcp, size as u64);
        assert!(report.error.is_none());
    }

    #[tokio::test]
    async fn reports_error_when_mix_side_breaks() {
        let (tcp_app, tcp_pumped) = tokio::io::duplex(1024);
        let (mix_pumped, mut mix_remote) = tokio::io::duplex(1024);
        let pumping = tokio::spawn(pump(tcp_pumped, mix_pumped, Arc::new(Stats::default())));
        mix_remote.write_all(&[42]).await.unwrap();
        let report = pumping.await.unwrap();
        assert!(report.error.unwrap().contains("unknown frame type"));
        drop(tcp_app);
    }
}
