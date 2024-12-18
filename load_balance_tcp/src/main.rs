use tokio::{
    io,
    net::{TcpListener, TcpStream},
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:9999").await?;
    let addrs = ["api01:9998", "api02:9997"];
    let mut count = 1;
    while let Ok((mut downstream, _)) = listener.accept().await {
        count += 1;
        let addr = addrs[count % addrs.len()];
        tokio::spawn(async move {
            let mut upstream = TcpStream::connect(addr).await.unwrap();
            io::copy_bidirectional(&mut downstream, &mut upstream)
                .await
                .unwrap();
        });
    }

    Ok(())
}
