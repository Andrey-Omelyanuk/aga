use super::*;

/// Распознать команду по первой строке сообщения. Не команда → None.
/// `#start` командой больше не является: нити начинаются только действием
/// у сообщения (см. историю message-threads).
pub fn parse_command(body: &str) -> Option<ChatCommand> {
    let first = body.lines().next()?.trim();
    let mut parts = first.split_whitespace();
    let cmd = parts.next()?;
    match cmd {
        "#invite" => parts.next().map(|n| ChatCommand::Invite(clean_at(n))),
        "#kick" => parts.next().map(|n| ChatCommand::Kick(clean_at(n))),
        "#end" => Some(ChatCommand::End),
        _ => None,
    }
}

fn clean_at(name: &str) -> String {
    name.trim_start_matches('@').to_string()
}

/// Упомянутые в тексте участники: слова вида `@имя` (открывающие скобки и
/// кавычки перед `@` и знаки препинания в конце слова не входят в имя). Каждое имя — один раз, в порядке появления.
pub fn mentioned_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_start_matches(|c: char| "(«\"'".contains(c));
        let Some(name) = word.strip_prefix('@') else {
            continue;
        };
        let name = name.trim_end_matches(|c: char| ".,:;!?)»\"'".contains(c));
        if !name.is_empty() && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// Найти сокращения, вызванные в тексте: слова вида `/имя`. Возвращает имена
/// без слэша, каждое один раз, в порядке первого появления. Имя не может быть
/// пустым и не содержит `/` (по слэшу в слове видно путь, а не сокращение).
pub fn shortcut_names(body: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for token in body.split_whitespace() {
        let Some(rest) = token.strip_prefix('/') else {
            continue;
        };
        let name = rest
            .trim_end_matches([',', '.', '!', '?', ':', ';'])
            .to_string();
        if name.is_empty() || name.contains('/') {
            continue;
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}
