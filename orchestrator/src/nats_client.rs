use nats::{Client, ConnectOptions};

pub struct NatsClient {
    client: Client,
}

impl NatsClient {
    pub async fn new(url: String) -> Result<Self, Box<dyn std::error::Error>> {
        let options = ConnectOptions::default()
            .reconnect_time_wait(1)
            .max_reconnect_attempts(-1); // Infinite reconnect attempts

        let client = Client::connect_with_options(&url, options).await?;

        Ok(NatsClient { client })
    }

    pub async fn subscribe(&self, subject: &str) -> Result<tokio::sync::mpsc::Receiver<Message>, Box<dyn std::error::Error>> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        // Spawn subscription task
        tokio::spawn(async move {
            let mut sub = match self.client.subscribe(subject).await {
                Ok(sub) => sub,
                Err(e) => {
                    eprintln!("Failed to subscribe to {}: {}", subject, e);
                    return;
                }
            };

            loop {
                match sub.next().await {
                    Some(Ok(msg)) => {
                        let _ = tx.send(msg).await;
                    }
                    Some(Err(e)) => {
                        eprintln!("Error receiving message: {}", e);
                    }
                    None => break,
                }
            }
        });

        Ok(rx)
    }

    pub async fn publish(&self, subject: &str, data: String) -> Result<(), Box<dyn std::error::Error>> {
        self.client.publish(subject, data).await?;
        Ok(())
    }
}
