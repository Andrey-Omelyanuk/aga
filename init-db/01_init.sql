-- Initialize database schema for Agent System

-- Create extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Agents table
CREATE TABLE IF NOT EXISTS agents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) UNIQUE NOT NULL,
    role VARCHAR(255) NOT NULL,
    description TEXT,
    capabilities JSONB DEFAULT '[]',
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

-- Tasks table
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

-- Task results table
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

-- Messages table for inter-agent communication history
CREATE TABLE IF NOT EXISTS messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_name VARCHAR(255) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    message_type VARCHAR(50) NOT NULL,
    payload JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Shared files metadata table
CREATE TABLE IF NOT EXISTS shared_files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    filename VARCHAR(1024) NOT NULL,
    file_size INTEGER,
    content_type VARCHAR(100),
    uploaded_by VARCHAR(255),
    uploaded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP,
    UNIQUE (filename)
);

-- Create indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_tasks_agent_status ON tasks(agent_name, status);
CREATE INDEX IF NOT EXISTS idx_task_results_task_id ON task_results(task_id);
CREATE INDEX IF NOT EXISTS idx_messages_agent_subject ON messages(agent_name, subject);
CREATE INDEX IF NOT EXISTS idx_shared_files_filename ON shared_files(filename);

-- Create function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Add trigger to agents table
DROP TRIGGER IF EXISTS update_agents_updated_at ON agents;
CREATE TRIGGER update_agents_updated_at
    BEFORE UPDATE ON agents
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Insert default agents (these will be overwritten by agent.md configs)
INSERT INTO agents (name, role, description, capabilities, status) VALUES
    ('code-analysis', 'Code Analysis Agent', 'Analyzes and understands code structure and semantics', '{"code_analysis", "understanding"}', 'idle'),
    ('code-generation', 'Code Generation Agent', 'Generates code based on requirements', '{"code_generation", "refactoring"}', 'idle'),
    ('code-review', 'Code Review Agent', 'Reviews code for quality and best practices', '{"code_review", "static_analysis"}', 'idle'),
    ('test-generation', 'Test Generation Agent', 'Generates unit and integration tests', '{"test_generation", "unit_testing"}', 'idle'),
    ('documentation', 'Documentation Agent', 'Generates documentation and README files', '{"documentation", "doc_generation"}', 'idle')
ON CONFLICT (name) DO NOTHING;
