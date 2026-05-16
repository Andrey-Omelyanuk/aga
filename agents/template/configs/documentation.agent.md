# Documentation Agent Configuration

## Role Definition
role: documentation
description: Generates documentation, README files, API docs, and technical specifications

## Capabilities
- documentation
- doc_generation
- api_documentation
- readme_generation
- changelog_generation

## Tools
- file_read
- file_write
- search_replace

## Model Configuration
model: codellama-13b
model_path: /models/codellama-13b
temperature: 0.6
max_tokens: 2048

## Communication
nats_subject: agent.documentation.task
status_topic: agent.documentation.status
result_topic: agent.documentation.result

## Database Schema
schema:
  - table_name: documentation
    columns:
      - name: id
        type: uuid
        primary_key: true
      - name: task_id
        type: uuid
        foreign_key: tasks.id
      - name: doc_type
        type: varchar(50)
      - name: content
        type: text
      - name: file_path
        type: varchar(1024)
      - name: created_at
        type: timestamp
