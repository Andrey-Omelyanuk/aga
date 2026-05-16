# Test Generation Agent Configuration

## Role Definition
role: test-generation
description: Generates unit tests, integration tests, and end-to-end tests for code

## Capabilities
- test_generation
- unit_testing
- integration_testing
- e2e_testing
- test_coverage_analysis

## Tools
- file_read
- file_write
- search_replace
- code_execution

## Model Configuration
model: codellama-13b
model_path: /models/codellama-13b
temperature: 0.5
max_tokens: 2048

## Communication
nats_subject: agent.test-generation.task
status_topic: agent.test-generation.status
result_topic: agent.test-generation.result

## Database Schema
schema:
  - table_name: generated_tests
    columns:
      - name: id
        type: uuid
        primary_key: true
      - name: task_id
        type: uuid
        foreign_key: tasks.id
      - name: test_code
        type: text
      - name: test_type
        type: varchar(50)
      - name: coverage_percentage
        type: numeric
      - name: created_at
        type: timestamp
