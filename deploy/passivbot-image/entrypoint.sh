#!/bin/bash
set -e

# ===== 参数读取与验证 =====
if [ -z "$BUCKET" ] || [ -z "$USER_ID" ] || [ -z "$BOT_ID" ]; then
  echo "[entrypoint][ERROR] Missing required environment variables." >&2
  echo "Required: BUCKET, USER_ID, BOT_ID" >&2
  exit 10
fi

CONFIG_DIR="/app/configs"
APP_DIR="/app"

mkdir -p "$CONFIG_DIR"

# 拼接 S3 路径
CONFIG_KEY="${USER_ID}/${BOT_ID}/${BOT_ID}.json"
API_KEY_KEY="${USER_ID}/${BOT_ID}/api-keys.json"

CONFIG_DEST="${CONFIG_DIR}/${BOT_ID}.json"
API_KEY_DEST="${APP_DIR}/api-keys.json"

echo "[entrypoint] BUCKET: $BUCKET"
echo "[entrypoint] USER_ID: $USER_ID"
echo "[entrypoint] BOT_ID: $BOT_ID"
echo "[entrypoint] downloading config.json from s3://${BUCKET}/${CONFIG_KEY} -> ${CONFIG_DEST}"
echo "[entrypoint] downloading api-keys.json from s3://${BUCKET}/${API_KEY_KEY} -> ${API_KEY_DEST}"

# ===== 拉取配置文件 =====
if ! aws s3 cp "s3://${BUCKET}/${CONFIG_KEY}" "$CONFIG_DEST"; then
  echo "[entrypoint][ERROR] failed to download s3://${BUCKET}/${CONFIG_KEY}" >&2
  exit 20
fi

if ! aws s3 cp "s3://${BUCKET}/${API_KEY_KEY}" "$API_KEY_DEST"; then
  echo "[entrypoint][ERROR] failed to download s3://${BUCKET}/${API_KEY_KEY}" >&2
  exit 21
fi

# 检查文件是否为空
if [ ! -s "$CONFIG_DEST" ] || [ ! -s "$API_KEY_DEST" ]; then
  echo "[entrypoint][ERROR] downloaded file is empty." >&2
  exit 22
fi

echo "[entrypoint] all config files downloaded successfully."

# ===== 启动主程序 =====
echo "[entrypoint] starting main app..."

exec python src/main.py "configs/${BOT_ID}.json"
