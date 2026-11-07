//! Сборка команд для `sh -c` (воркстейшн, песочница, служебные команды ядра).
//! Любое значение, которое подставляется в команду, — только через [`quote`]:
//! пути, ветки, git-URL и т.п. приходят из API и не должны ломать разбор
//! команды.

/// Строка в одинарных кавычках для sh: внутренняя `'` закрывает кавычки,
/// экранируется и открывает их снова (`it's` → `'it'\''s'`).
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_inner_quotes() {
        assert_eq!(quote("plain"), "'plain'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("$(id); `id`"), "'$(id); `id`'");
    }

    /// Значение в кавычках доходит до команды буквально — sh его не исполняет.
    #[test]
    fn quoted_value_is_not_executed_by_sh() {
        let evil = "x'; echo pwned; echo '";
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {}", quote(evil)))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), evil);
    }
}
