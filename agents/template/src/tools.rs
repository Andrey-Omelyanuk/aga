use std::sync::Arc;
use tokio::sync::Mutex;

use super::{FileTools, CodeTools};

pub struct FileTools {
    shared_path: String,
}

impl FileTools {
    pub fn new(shared_path: &str) -> Self {
        FileTools {
            shared_path: shared_path.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl FileTools for FileTools {
    async fn read_file(&self, path: &str) -> Result<String, String> {
        let full_path = format!("{}/{}", self.shared_path, path);
        
        match std::fs::read_to_string(&full_path) {
            Ok(content) => Ok(content),
            Err(e) => Err(format!("Failed to read file {}: {}", full_path, e)),
        }
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<(), String> {
        let full_path = format!("{}/{}", self.shared_path, path);
        
        match std::fs::write(&full_path, content) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to write file {}: {}", full_path, e)),
        }
    }

    async fn list_directory(&self, path: &str) -> Result<Vec<String>, String> {
        let full_path = format!("{}/{}", self.shared_path, path);
        
        match walkdir::WalkDir::new(&full_path)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .map(|e| e.path().to_string_lossy().to_string())
            .collect() {
            Ok(entries) => Ok(entries),
            Err(e) => Err(format!("Failed to list directory {}: {}", full_path, e)),
        }
    }
}

pub struct CodeTools {
    file_tools: Arc<Mutex<dyn FileTools>>,
}

impl CodeTools {
    pub fn new(file_tools: Arc<Mutex<dyn FileTools>>) -> Self {
        CodeTools { file_tools }
    }
}

#[async_trait::async_trait]
impl CodeTools for CodeTools {
    async fn execute_code(&self, code: &str) -> Result<String, String> {
        // In production, this would execute the code in a sandboxed environment
        // For now, return a placeholder
        
        Ok(format!("Code execution result:\n{}", code))
    }

    async fn search_replace(&self, file_path: &str, old_text: &str, new_text: &str) -> Result<(), String> {
        // Read the file
        let content = self.file_tools.lock().await.read_file(file_path).await?;
        
        // Perform replacement
        let new_content = content.replace(old_text, new_text);
        
        // Write back to file
        self.file_tools.lock().await.write_file(file_path, &new_content).await?;
        
        Ok(())
    }

    async fn analyze_code(&self, code: &str) -> Result<String, String> {
        // In production, this would use static analysis tools
        // For now, return a placeholder
        
        Ok(format!("Code analysis result for:\n{}", code))
    }
}
