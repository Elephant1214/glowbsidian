use glowbsidian_raknet::Event;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let cfg = glowbsidian_raknet::Config {
        protocols: vec![8],
        ..Default::default()
    };

    let mut listener = glowbsidian_raknet::Listener::bind(cfg.clone()).await?;
    listener.set_pong(format!(
        "MCPE;Test Server;92;0.17.2;0;25;{};§6Cool server§r;Survival;",
        cfg.guid,
    ));

    while let Some(event) = listener.next_event().await {
        match event {
            Event::Connected {
                peer,
                addr,
                protocol,
            } => {
                tracing::info!(?peer, %addr, %protocol, "New connection");
            }
            Event::Message { peer, data } => {
                let data = &data[..];
                tracing::info!("Incoming game packet from {peer:?}: {data:02x?}");
            }
            Event::Disconnected { peer, reason } => {
                tracing::info!(?peer, ?reason, "Player disconnected");
            }
        }
    }

    Ok(())
}
