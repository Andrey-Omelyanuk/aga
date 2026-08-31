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

/// Развернуть значение приватного ключа в пригодный для OpenSSH вид.
/// Литералы `\n` -> переводы строк (в `.env` многострочный ключ записывается
/// одной строкой; `make` не умеет include многострочные значения); реальные
/// переносы оставляются как есть. OpenSSH требует, чтобы строка
/// `-----END OPENSSH PRIVATE KEY-----` завершалась переводом строки — в
/// `.env` финального `\n` после END нет, поэтому гарантируем завершающий `\n`.
fn unfold_private_key(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        let unfolded = value.replace("\\n", "\n");
        Some(if unfolded.ends_with('\n') {
            unfolded
        } else {
            format!("{unfolded}\n")
        })
    }
}

/// Приватный ключ из env, если задан и не пуст.
pub fn private_key_from_env() -> Option<String> {
    std::env::var(SSH_PRIVATE_KEY_ENV)
        .ok()
        .and_then(|value| unfold_private_key(&value))
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
        let value = unfold_private_key(
            "-----BEGIN OPENSSH PRIVATE KEY-----\\nline\\n-----END OPENSSH PRIVATE KEY-----",
        )
        .unwrap();
        assert!(value.contains("KEY-----\nline\n-----END"));
    }

    #[test]
    fn keeps_real_newlines_untouched() {
        // Гарантированный завершающий `\n` (требование OpenSSH) тоже добавлен.
        assert_eq!(unfold_private_key("a\nb").unwrap(), "a\nb\n");
    }

    #[test]
    fn private_key_always_ends_with_newline() {
        // В `.env` ключ одной строкой, без `\n` после END: OpenSSH не грузит
        // ключ без завершающего перевода строки ("error in libcrypto").
        let value = unfold_private_key(
            "-----BEGIN OPENSSH PRIVATE KEY-----\\nline\\n-----END OPENSSH PRIVATE KEY-----",
        )
        .unwrap();
        assert!(value.ends_with("KEY-----\n"));
    }

    #[test]
    fn empty_env_value_means_not_configured() {
        assert!(unfold_private_key("  ").is_none());
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
