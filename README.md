# PBTB-Rust

A Rust project with DynamoDB integration.

## Prerequisites

- Rust (latest stable version)
- Docker and Docker Compose
- AWS CLI (for local DynamoDB interaction)

## Setup

1. Clone the repository:
   ```
   git clone <repository-url>
   cd pbtb-rust
   ```

2. Start the local DynamoDB instance:
   ```
   docker-compose up -d
   ```

3. Verify that the DynamoDB instance is running:
   ```
   aws dynamodb list-tables --endpoint-url http://localhost:8000
   ```

## Configuration

The application uses a configuration file located at `config/default.toml`. You can override these settings by creating a `config/local.toml` file or by setting environment variables with the prefix `APP__`.

Example configuration:
```toml
[dynamodb]
endpoint_url = "http://localhost:8000"
region = "us-east-1"
table_name = "bots"
```

## Running Tests

### Using the Convenience Script

The easiest way to run the tests is to use the provided script:

```
./run_tests.sh
```

This script will:
1. Check if Docker is running
2. Start the local DynamoDB instance if it's not already running
3. Verify that the DynamoDB instance is accessible
4. Run the tests

### Manual Testing

Alternatively, you can run the tests manually:

1. Make sure the local DynamoDB instance is running:
   ```
   docker-compose up -d
   ```

2. Run the tests:
   ```
   cargo test
   ```

The tests will automatically create the necessary tables in the local DynamoDB instance.

### Test Structure

The tests for the `botrepository.rs` file are located in the `tests/botrepository_test.rs` file. They test the following functionality:

1. Saving a bot and finding it by ID
2. Finding a bot with a non-existent ID
3. Updating an existing bot

## Project Structure

- `src/domain/bot.rs`: Domain model for bots
- `src/infra/botrepository.rs`: DynamoDB implementation of the bot repository
- `src/config.rs`: Configuration loading
- `src/utils.rs`: Utility functions for DynamoDB setup
- `tests/`: Test files

## Development

To add a new feature:

1. Add the domain model to the `domain` directory
2. Implement the repository in the `infra` directory
3. Add tests to the `tests` directory
4. Update the configuration if necessary
