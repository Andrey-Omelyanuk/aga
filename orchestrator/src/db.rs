use sqlx::PgPool;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct AgentConfig {
    pub name: String,
    pub role: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub model: ModelConfig,
    pub nats_subject: String,
    pub status_topic: String,
    pub result_topic: String,
}

#[derive(Debug, Serialize)]
pub struct ModelConfig {
    pub name: String,
    pub path: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

pub async fn create_tables(pool: &PgPool) -> Result<(), sqlx::Error> {
    let sql = r#"
    CREATE TABLE IF NOT EXISTS agents (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        name VARCHAR(255) UNIQUE NOT NULL,
        role VARCHAR(255) NOT NULL,
        description TEXT,
        capabilities JSONB,
        model_name VARCHAR(255),
        model_path VARCHAR(1024),
        temperature FLOAT,
        max_tokens INTEGER,
        nats_subject VARCHAR(255),
        status_topic VARCHAR(255),
        result_topic VARCHAR(255),
        status VARCHAR(50) DEFAULT 'idle',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS tasks (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        agent_name VARCHAR(255) NOT NULL,
        task_description TEXT NOT NULL,
        context TEXT,
        priority INTEGER,
        status VARCHAR(50) DEFAULT 'pending',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        completed_at TIMESTAMP,
        FOREIGN KEY (agent_name) REFERENCES agents(name)
    );

    CREATE TABLE IF NOT EXISTS task_results (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        task_id UUID NOT NULL,
        agent_name VARCHAR(255) NOT NULL,
        result TEXT,
        error TEXT,
        execution_time_ms INTEGER,
        status VARCHAR(50),
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (task_id) REFERENCES tasks(id)
    );

    CREATE TABLE IF NOT EXISTS messages (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        agent_name VARCHAR(255) NOT NULL,
        subject VARCHAR(255) NOT NULL,
        message_type VARCHAR(50) NOT NULL,
        payload JSONB,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    );

    CREATE INDEX IF NOT EXISTS idx_tasks_agent_status ON tasks(agent_name, status);
    CREATE INDEX IF NOT EXISTS idx_task_results_task_id ON task_results(task_id);
    "#;

    pool.execute(sql).await?;
    Ok(())
}

pub async fn save_agent_config(pool: &PgPool, name: &str, config: &AgentConfig) -> Result<(), sqlx::Error> {
    let capabilities_json = serde_json::to_string(&config.capabilities)?;
    
    let sql = r#"
    INSERT INTO agents (name, role, description, capabilities, model_name, model_path, 
                        temperature, max_tokens, nats_subject, status_topic, result_topic)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
    ON CONFLICT (name) DO UPDATE SET
        role = EXCLUDED.role,
        description = EXCLUDED.description,
        capabilities = EXCLUDED.capabilities,
        model_name = EXCLUDED.model_name,
        model_path = EXCLUDED.model_path,
        temperature = EXCLUDED.temperature,
        max_tokens = EXCLUDED.max_tokens,
        nats_subject = EXCLUDED.nats_subject,
        status_topic = EXCLUDED.status_topic,
        result_topic = EXCLUDED.result_topic,
        updated_at = CURRENT_TIMESTAMP
    "#;

    sqlx::query_as::<_, ()>(sql)
        .bind(name)
        .bind(&config.role)
        .bind(&config.description)
        .bind(capabilities_json)
        .bind(&config.model.name)
        .bind(&config.model.path)
        .bind(config.model.temperature)
        .bind(config.model.max_tokens)
        .bind(&config.nats_subject)
        .bind(&config.status_topic)
        .bind(&config.result_topic)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn get_agent_config(pool: &PgPool, name: &str) -> Option<AgentConfig> {
    let sql = r#"
    SELECT 
        name, role, description, capabilities::jsonb as capabilities,
        model_name as model_name, model_path as model_path,
        temperature, max_tokens, nats_subject, status_topic, result_topic
    FROM agents
    WHERE name = $1
    "#;

    match sqlx::query_as::<_, (String, String, String, Vec<String>, String, String, f32, i32, String, String, String)>(sql)
        .bind(name)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => Some(AgentConfig {
            name: row.0,
            role: row.1,
            description: row.2,
            capabilities: row.3,
            model: ModelConfig {
                name: row.4,
                path: row.5,
                temperature: row.6,
                max_tokens: row.7,
            },
            nats_subject: row.8,
            status_topic: row.9,
            result_topic: row.10,
        }),
        Err(_) => None,
    }
}
