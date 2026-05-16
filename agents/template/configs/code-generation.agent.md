# Code Generation Agent Configuration

## Role Definition
role: code-generation
description: Generates code based on requirements and specifications, supports multiple programming languages

## Capabilities
- code_generation
- refactoring
- optimization
- multi_language_support

## Tools
- file_read
- file_write
- search_replace
- code_execution

## Model Configuration
model: codellama-13b
model_path: /models/codellama-13b
temperature: 0.7
max_tokens: 4096

## Communication
nats_subject: agent.code-generation.task
status_topic: agent.code-generation.status
result_topic: agent.code-generation.result

## Database Schema
schema:
  - table_name: generated_code
    columns:
      - name: id
        type: uuid
        primary_key: true
      - name: task_id
        type: uuid
        foreign_key: tasks.id
      - name: code_content
        type: text
      - name: file_path
        type: varchar(1024)
      - name: language
        type: varchar(100)
      - name: created_at
        type: timestamp
