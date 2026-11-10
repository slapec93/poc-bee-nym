use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const DATA: u8 = 0;
const FIN: u8 = 1;
pub const MAX_PAYLOAD: usize = 32 * 1024;

#[derive(Debug, PartialEq)]
pub enum Frame {
    Data(Vec<u8>),
    Fin,
}

pub async fn write_data<W: AsyncWrite + Unpin>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(DATA);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    w.write_all(&buf).await?;
    w.flush().await
}

pub async fn write_fin<W: AsyncWrite + Unpin>(w: &mut W) -> std::io::Result<()> {
    w.write_all(&[FIN]).await?;
    w.flush().await
}

pub async fn read<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Frame> {
    match r.read_u8().await? {
        DATA => {
            let len = r.read_u32().await? as usize;
            if len > MAX_PAYLOAD {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("frame too large: {len}")));
            }
            let mut payload = vec![0u8; len];
            r.read_exact(&mut payload).await?;
            Ok(Frame::Data(payload))
        }
        FIN => Ok(Frame::Fin),
        t => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unknown frame type {t}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip() {
        let (mut a, mut b) = tokio::io::duplex(1 << 20);
        write_data(&mut a, b"hello").await.unwrap();
        write_data(&mut a, &[]).await.unwrap();
        write_data(&mut a, &vec![7u8; MAX_PAYLOAD]).await.unwrap();
        write_fin(&mut a).await.unwrap();
        assert_eq!(read(&mut b).await.unwrap(), Frame::Data(b"hello".to_vec()));
        assert_eq!(read(&mut b).await.unwrap(), Frame::Data(vec![]));
        assert_eq!(read(&mut b).await.unwrap(), Frame::Data(vec![7u8; MAX_PAYLOAD]));
        assert_eq!(read(&mut b).await.unwrap(), Frame::Fin);
    }

    #[tokio::test]
    async fn rejects_oversized_and_unknown() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&[DATA]).await.unwrap();
        a.write_all(&((MAX_PAYLOAD + 1) as u32).to_be_bytes()).await.unwrap();
        assert!(read(&mut b).await.is_err());

        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&[9]).await.unwrap();
        assert!(read(&mut b).await.is_err());
    }
}
