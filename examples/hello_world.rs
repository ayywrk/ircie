use ircie::{system::IntoResponse, Irc, IrcPrefix};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut irc = Irc::from_config("irc_config.yaml").await?;

    irc.add_system("hello", hello_world).await;

    irc.run().await
}

fn hello_world(prefix: IrcPrefix) -> impl IntoResponse {
    format!("Hello {}!", prefix.nick)
}
