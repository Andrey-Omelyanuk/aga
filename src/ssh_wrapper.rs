use async_trait::async_trait;
use russh::{client, ChannelMsg};
use russh_keys::key::KeyPair;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::TargetConfig;

pub struct SshSession {
    session: Option<client::Handle<SshHandler>>,
    config: TargetConfig,
}

struct SshHandler;

#[async_trait]
impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        self,
        _server_public_key: &russh_keys::key::PublicKey,
    ) -> Result<(Self, bool), Self::Error> {
        // В продакшене можно добавить проверку known_hosts
        Ok((self, true))
    }
}

impl SshSession {
    pub fn new(config: TargetConfig) -> Self {
        Self {
            session: None,
            config,
        }
    }

    pub async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let key_path = self.config.key_path.to_string_lossy().to_string();
        let key_pair = KeyPair::read_openssh_file(&key_path)?;

        let config = russh::client::Config {
            ..Default::default()
        };

        let sh = SshHandler;
        
        let mut session = russh::client::connect(
            Arc::new(config),
            (self.config.host.as_str(), self.config.port),
            sh,
        )
        .await?;

        if session.authenticate_password(self.config.user.as_str(), "").await?.is_success() {
            // Аутентификация паролем не нужна, пробуем ключ
        }

        if session.authenticate_publickey(self.config.user.as_str(), Arc::new(key_pair)).await?.is_success() {
            self.session = Some(session);
            Ok(())
        } else {
            Err("SSH authentication failed".into())
        }
    }

    pub async fn execute(&mut self, command: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if self.session.is_none() {
            self.connect().await?;
        }

        let session = self.session.as_mut().unwrap();
        let mut channel = session.channel_open_session().await?;
        
        channel.exec(true, command).await?;
        
        let mut output = String::new();
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    output.push_str(&String::from_utf8_lossy(&data));
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    if exit_status != 0 {
                        return Err(format!("Command exited with status {}", exit_status).into());
                    }
                    break;
                }
                ChannelMsg::Closed => break,
                _ => {}
            }
        }

        Ok(output)
    }

    pub async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(mut session) = self.session.take() {
            session.disconnect().await?;
        }
        Ok(())
    }
}

pub struct SshPool {
    sessions: Mutex<std::collections::HashMap<String, SshSession>>,
}

impl SshPool {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub async fn get_or_create(
        &self,
        key: &str,
        config: TargetConfig,
    ) -> Result<tokio::sync::MutexGuard<'_, SshSession>, Box<dyn std::error::Error + Send + Sync>> {
        let mut sessions = self.sessions.lock().await;
        
        if !sessions.contains_key(key) {
            let mut session = SshSession::new(config);
            session.connect().await?;
            sessions.insert(key.to_string(), session);
        }

        // Нужно вернуть сессию, но это сложно с MutexGuard
        // Упростим для базовой версии
        drop(sessions);
        Err("Session pool not fully implemented".into())
    }
}
