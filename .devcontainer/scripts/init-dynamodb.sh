#!/usr/bin/env bash
set -euo pipefail

ENDPOINT="${APP__DYNAMODB__ENDPOINT_URL:-http://dynamodb-local:8000}"
TABLE_DEF_PATH="/app/.devcontainer/infra/bots.json"
TABLE_NAME="bots"

echo "[init-dynamodb] waiting for DynamoDB Local at $ENDPOINT ..."
# wait for DynamoDB Local
for i in {1..60}; do
  if curl -s "$ENDPOINT/shell/" >/dev/null 2>&1 || curl -s "$ENDPOINT" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "[init-dynamodb] ensuring table '$TABLE_NAME' exists ..."
if aws dynamodb describe-table \
      --endpoint-url "$ENDPOINT" \
      --table-name "$TABLE_NAME" >/dev/null 2>&1; then
  echo "[init-dynamodb] table already exists."
else
  echo "[init-dynamodb] creating table from $TABLE_DEF_PATH ..."
  aws dynamodb create-table \
    --endpoint-url "$ENDPOINT" \
    --cli-input-json "file://$TABLE_DEF_PATH"

  echo "[init-dynamodb] waiting for table ACTIVE ..."
  aws dynamodb wait table-exists --endpoint-url "$ENDPOINT" --table-name "$TABLE_NAME"

  # option：enable PITR / TTL
  # aws dynamodb update-continuous-backups --endpoint-url "$ENDPOINT" --table-name "$TABLE_NAME" --point-in-time-recovery-specification PointInTimeRecoveryEnabled=true
  # aws dynamodb update-time-to-live --endpoint-url "$ENDPOINT" --table-name "$TABLE_NAME" --time-to-live-specification "Enabled=true,AttributeName=ttl"
fi

echo "[init-dynamodb] done."
