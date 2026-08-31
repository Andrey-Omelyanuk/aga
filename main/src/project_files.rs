use crate::agent::{execute_via_executor, Executor};
use base64::Engine;
use serde::Serialize;

/// Путь к проекту внутри воркстейшна (под/контейнер): жёстко зашит, общий
/// для k8s и docker-бэкенда (см. cluster.rs).
pub const PROJECT_ROOT: &str = "/work/project";

/// Ошибка просмотра содержимого проекта.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("недопустимый путь: {0}")]
    InvalidPath(String),
    #[error("путь не найден")]
    NotFound,
    #[error("исполнение команды в воркстейшне не удалось: {0}")]
    Exec(String),
}

impl From<Box<dyn std::error::Error + Send + Sync>> for FileError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        FileError::Exec(e.to_string())
    }
}

/// Запись дерева: файл или папка.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    /// Путь относительно корня проекта.
    pub path: String,
    /// "dir" | "file".
    pub kind: String,
}

/// Содержимое папки (лениво, по запрошенному пути).
#[derive(Debug, Clone, Serialize)]
pub struct Tree {
    pub path: String,
    pub entries: Vec<FileEntry>,
}

/// Содержимое файла: байты и MIME-тип. Решение «текст или медиа» принимает
/// вызывающий по MIME.
#[derive(Debug, Clone)]
pub struct FileContent {
    pub bytes: Vec<u8>,
    pub mime: String,
}

/// Очистить относительный путь и отклонить обход корня / инъекцию аргументов.
/// Путь от корня проекта (под/контейнер). Одиночная кавычка и слеш в начале
/// запрещены — команды собираются как `... '<rel>'` и не должны вырваться.
pub fn sanitize_rel(path: &str) -> Result<String, FileError> {
    let trimmed = path.trim();
    if trimmed == "." || trimmed.is_empty() {
        return Ok(String::new());
    }
    // Абсолютный путь (выход из /work/project) отклоняем целиком.
    if trimmed.starts_with('/') || trimmed.contains('\'') {
        return Err(FileError::InvalidPath(path.to_string()));
    }
    let cleaned = trimmed.trim_matches('/');
    if cleaned.is_empty() || cleaned.starts_with("~/") {
        return Err(FileError::InvalidPath(path.to_string()));
    }
    for seg in cleaned.split('/') {
        // "..", пустой (двойной слеш) и скрытые системные пути отклоняем.
        if seg == ".." || seg.is_empty() {
            return Err(FileError::InvalidPath(path.to_string()));
        }
    }
    Ok(cleaned.to_string())
}

/// Команды листинга папки. GNU-`find -printf` в BusyBox (alpine, образ
/// воркстейшна `docker:dind`) отсутствует, поэтому типы получаем двумя
/// проходами: папки (`-type d`) и файлы (`-type f`). `-maxdepth/-mindepth/
/// -type/-print` есть и в GNU, и в BusyBox — команда переносима между ними.
fn listing_commands(root: &str, rel: &str) -> [String; 2] {
    let target = join_root(root, rel);
    [
        format!("find '{}' -maxdepth 1 -mindepth 1 -type d -print", target),
        format!("find '{}' -maxdepth 1 -mindepth 1 -type f -print", target),
    ]
}

/// Команда чтения файла: base64 (бинарно-безопасно для текста и медиа),
/// декод делает ядро.
fn read_command(root: &str, rel: &str) -> String {
    let target = join_root(root, rel);
    format!("base64 '{}'", target)
}

fn join_root(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        root.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel)
    }
}

/// Ошибка исполнения, при которой воркстейшн запустил команду, но путь
/// отсутствует (find/base64 вернули не-ноль с «no such file»), — 404. Сбой
/// самого исполнения (бинарщина kubectl/docker недоступна) — Exec (500).
fn exec_error_or_not_found(e: Box<dyn std::error::Error + Send + Sync>) -> FileError {
    let msg = e.to_string();
    if msg.starts_with("Command failed:")
        && (msg.contains("No such file")
            || msg.contains("not found")
            || msg.contains("cannot open"))
    {
        FileError::NotFound
    } else {
        FileError::Exec(msg)
    }
}

/// MIME по расширению: медиа (картинки/видео/аудио), иначе текст.
/// Неизвестные расширения — как бинарь (octet-stream), текст не гадаем.
pub fn mime_for(path: &str) -> String {
    let ext = path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let base = match ext.as_str() {
        // Картинки.
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        // Видео.
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "ogv" => "video/ogg",
        // Аудио.
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        "m4a" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        // Текстовые — подсветка синтаксиса на фронте по расширению.
        "rs" | "go" | "py" | "js" | "ts" | "c" | "h" | "cpp" | "hpp" | "java" | "kt" | "rb"
        | "php" | "sh" | "bash" | "html" | "htm" | "css" | "scss" | "json" | "yaml" | "yml"
        | "toml" | "ini" | "xml" | "md" | "markdown" | "txt" | "sql" | "graphql" | "proto"
        | "dockerfile" | "makefile" => "text/plain; charset=utf-8",
        // Неизвестное — бинарь (текст не гадаем).
        _ => "application/octet-stream",
    };
    base.to_string()
}

/// Разобрать строки `find -print` (по одному пути в строке) в записи дерева
/// заданного типа (`kind`).
fn parse_find_lines(out: &str, root: &str, kind: &str, entries: &mut Vec<FileEntry>) {
    for line in out.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let full = line.trim_end_matches('/');
        if full.is_empty() {
            continue;
        }
        let name = full.rsplit('/').next().unwrap_or(full).to_string();
        let rel_path = full
            .strip_prefix(root.trim_end_matches('/'))
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or(full);
        entries.push(FileEntry {
            name,
            path: rel_path.to_string(),
            kind: kind.to_string(),
        });
    }
}

/// Список папки. `root` — путь к проекту на машине-воркстейшне (в проде
/// PROJECT_ROOT); rel — относительный (санитизированный) путь.
pub async fn tree(executor: &Executor, root: &str, rel: &str) -> Result<Tree, FileError> {
    let rel = sanitize_rel(rel)?;
    let mut entries: Vec<FileEntry> = Vec::new();
    for (command, kind) in listing_commands(root, &rel).into_iter().zip(["dir", "file"]) {
        let out = match execute_via_executor(executor, &command).await {
            Ok(out) => out,
            Err(e) => return Err(exec_error_or_not_found(e)),
        };
        parse_find_lines(&out, root, kind, &mut entries);
    }
    entries.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Tree { path: rel, entries })
}

/// Прочитать файл: байты + MIME по расширению. 404, если путь не существует.
pub async fn read(executor: &Executor, root: &str, rel: &str) -> Result<FileContent, FileError> {
    let rel = sanitize_rel(rel)?;
    if rel.is_empty() {
        return Err(FileError::InvalidPath(rel));
    }
    let out = match execute_via_executor(executor, &read_command(root, &rel)).await {
        Ok(out) => out,
        Err(e) => return Err(exec_error_or_not_found(e)),
    };
    // `base64` CLI оборачивает вывод по 76 символов; декодер crate требует
    // сплошную строку — убираем все пробельные символы.
    let b64 = out.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .map_err(|e| FileError::Exec(format!("base64 decode: {e}")))?;
    Ok(FileContent {
        bytes,
        mime: mime_for(&rel),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(root.join("src/pkg")).unwrap();
        std::fs::write(root.join("README.md"), "# Проект\n\nПривет").unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() { println!(\"hi\"); }\n",
        )
        .unwrap();
        std::fs::write(root.join("src/pkg/mod.rs"), "pub fn f() {}\n").unwrap();
        std::fs::write(
            root.join("img.png"),
            [0x89, 0x50, 0x4e, 0x47, 0x01, 0x02, 0x03],
        )
        .unwrap();
        std::fs::write(root.join("clip.mp4"), [0x00, 0x00, 0x00, 0x18]).unwrap();
        std::fs::write(root.join("song.mp3"), [0xff, 0xfb, 0x90, 0x00]).unwrap();
        (dir, root)
    }

    #[tokio::test]
    async fn project_tree_lists_files_and_folders() {
        let (_dir, root) = temp_project();
        let t = tree(&Executor::Sh, root.to_str().unwrap(), "")
            .await
            .unwrap();
        let names: Vec<&str> = t.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"src"));
        assert!(names.contains(&"README.md"));
        // Папки идут первыми; у папки src относительный путь src.
        assert_eq!(t.entries[0].kind, "dir");
        assert!(t.entries.iter().any(|e| e.path == "src"));
        let t = tree(&Executor::Sh, root.to_str().unwrap(), "src")
            .await
            .unwrap();
        assert!(t.entries.iter().any(|e| e.name == "main.rs"));
        assert!(t.entries.iter().any(|e| e.path == "src/pkg"));
    }

    #[tokio::test]
    async fn text_file_read_returns_content() {
        let (_dir, root) = temp_project();
        let f = read(&Executor::Sh, root.to_str().unwrap(), "src/main.rs")
            .await
            .unwrap();
        assert_eq!(&f.mime, "text/plain; charset=utf-8");
        let text = String::from_utf8(f.bytes).unwrap();
        assert!(text.contains("fn main"));
    }

    #[tokio::test]
    async fn large_text_file_with_wrapped_base64_reads_correctly() {
        // Файл больше 57 байт: `base64` CLI оборачивает вывод по 76 символов,
        // декодер должен собирать его из нескольких строк.
        let (_dir, root) = temp_project();
        let big = "строка для теста оборачивания base64. ".repeat(20);
        std::fs::write(root.join("big.txt"), &big).unwrap();
        let f = read(&Executor::Sh, root.to_str().unwrap(), "big.txt")
            .await
            .unwrap();
        assert_eq!(String::from_utf8(f.bytes).unwrap(), big);
    }

    #[tokio::test]
    async fn image_file_read_returns_bytes() {
        let (_dir, root) = temp_project();
        let f = read(&Executor::Sh, root.to_str().unwrap(), "img.png")
            .await
            .unwrap();
        assert_eq!(&f.mime, "image/png");
        assert_eq!(f.bytes, [0x89, 0x50, 0x4e, 0x47, 0x01, 0x02, 0x03]);
    }

    #[tokio::test]
    async fn video_and_audio_file_read_returns_bytes() {
        let (_dir, root) = temp_project();
        let v = read(&Executor::Sh, root.to_str().unwrap(), "clip.mp4")
            .await
            .unwrap();
        assert_eq!(v.mime, "video/mp4");
        let a = read(&Executor::Sh, root.to_str().unwrap(), "song.mp3")
            .await
            .unwrap();
        assert_eq!(a.mime, "audio/mpeg");
    }

    #[tokio::test]
    async fn missing_file_returns_error() {
        let (_dir, root) = temp_project();
        let res = read(&Executor::Sh, root.to_str().unwrap(), "nope.txt").await;
        assert!(res.is_err());
    }

    #[test]
    fn sanitize_rel_rejects_path_traversal_and_shell_breaks() {
        assert!(sanitize_rel("../etc/passwd").is_err());
        assert!(sanitize_rel("a/../../b").is_err());
        assert!(sanitize_rel("/etc/passwd").is_err());
        assert!(sanitize_rel("src/'/x").is_err());
        assert!(sanitize_rel("src/main.rs").is_ok());
        assert!(sanitize_rel("").is_ok());
    }

    #[test]
    fn mime_detected_by_extension() {
        assert_eq!(mime_for("pic.PNG"), "image/png");
        assert_eq!(mime_for("movie.mp4"), "video/mp4");
        assert_eq!(mime_for("audio.mp3"), "audio/mpeg");
        assert_eq!(mime_for("code.rs"), "text/plain; charset=utf-8");
        assert_eq!(mime_for("blob.bin"), "application/octet-stream");
        assert_eq!(mime_for("noext"), "application/octet-stream");
    }
}
