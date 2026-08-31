//! SSH-ключ aga (общий на инстанс). Приватный ключ задаёт админ в env
//! `AGA_SSH_PRIVATE_KEY` (OpenSSH-формат, `ssh-keygen`); ядро вычисляет из
//! него публичный для отдачи клиенту и прокидывает приватный в воркстейшны
//! (git+ssh-доступ: клон и push проектов).

use ssh_key::{HashAlg, PrivateKey};

/// Имя env-переменной с приватным ключом (OpenSSH-формат, `-----BEGIN OPENSSH
/// PRIVATE KEY-----`). Задаётся админом на уровне окружения ядра.
pub const SSH_PRIVATE_KEY_ENV: &str = "AGA_SSH_PRIVATE_KEY";

/// Имя k8s-Secret, в который ядро складывает ключ для монтирования в поды
/// воркстейшнов (см. `workstations.secret`).
pub const SSH_SECRET_NAME: &str = "aga-ssh";

/// Приватный ключ из env, если задан и не пуст. Литералы `\n` разворачиваются
/// в переносы строк: в `.env` многострочный ключ записывается одной строкой
/// (`make` не умеет include многострочные значения); реальные переносы тоже
/// допустимы (docker/k8s env) — тогда изменений нет.
pub fn private_key_from_env() -> Option<String> {
    let value = std::env::var(SSH_PRIVATE_KEY_ENV).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.replace("\\n", "\n"))
    }
}

/// Публичный ключ и SHA256-fingerprint из приватного (OpenSSH-формат).
pub fn derive_public_key(
    private_pem: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let key: PrivateKey = private_pem.parse()?;
    let public = key.public_key();
    let public_key = public.to_openssh()?;
    let fingerprint = public.fingerprint(HashAlg::Sha256).to_string();
    Ok((public_key, fingerprint))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACDA17kzo52uu2+R1RFiQdTtrlrgb25kAFtqD1mLc9/sVgAAAIhgXrj5YF64
+QAAAAtzc2gtZWQyNTUxOQAAACDA17kzo52uu2+R1RFiQdTtrlrgb25kAFtqD1mLc9/sVg
AAAECAcToP9c1wkXJYjZAUT7Mg+0dhlhbJDxIyyXvEAUGHAcDXuTOjna67b5HVEWJB1O2u
WuBvbmQAW2oPWYtz3+xWAAAABHRlc3QB
-----END OPENSSH PRIVATE KEY-----
";

    #[test]
    fn env_absent_means_not_configured() {
        std::env::remove_var(SSH_PRIVATE_KEY_ENV);
        assert!(private_key_from_env().is_none());
    }

    #[test]
    fn unfolds_literal_newlines_from_single_line_env() {
        std::env::set_var(
            SSH_PRIVATE_KEY_ENV,
            "-----BEGIN OPENSSH PRIVATE KEY-----\\nline\\n-----END OPENSSH PRIVATE KEY-----",
        );
        let value = private_key_from_env().unwrap();
        assert!(value.contains("KEY-----\nline\n-----END"));
        std::env::remove_var(SSH_PRIVATE_KEY_ENV);
    }

    #[test]
    fn keeps_real_newlines_untouched() {
        std::env::set_var(SSH_PRIVATE_KEY_ENV, "a\nb");
        let value = private_key_from_env().unwrap();
        assert_eq!(value, "a\nb");
        std::env::remove_var(SSH_PRIVATE_KEY_ENV);
    }

    #[test]
    fn derives_public_key_and_fingerprint() {
        let (public, fingerprint) = derive_public_key(TEST_PRIVATE).unwrap();
        assert!(public.starts_with("ssh-ed25519 "));
        assert!(fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn rejects_garbage_key() {
        assert!(derive_public_key("not a key").is_err());
    }
}
