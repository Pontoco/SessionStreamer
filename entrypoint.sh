#!/bin/sh
set -e # Exit immediately if a command exits with a non-zero status.

# Environment variables (GCS_BUCKET_NAME and APP_DATA_DIR_NAME are expected to be set)
# PORT is set by Cloud Run

GCS_MOUNT_POINT="/mnt/gcs-bucket"
# Construct the full path that will be passed to your application
APP_INTERNAL_DATA_PATH="${GCS_MOUNT_POINT}/${APP_DATA_DIR_NAME}"

if [ -z "$GCS_BUCKET_NAME" ]; then
  echo "Error: GCS_BUCKET_NAME environment variable is not set."
  exit 1
fi

# Ensure the FUSE mount point directory exists
mkdir -p "$GCS_MOUNT_POINT"

echo "Mounting GCS bucket '$GCS_BUCKET_NAME' to '$GCS_MOUNT_POINT'..."
# Mount gcsfuse.
# --implicit-dirs: Allows treating GCS paths like directories even if they don't exist as objects.
# For Cloud Run, gcsfuse will use the attached service account for authentication by default.
# The gcsfuse command itself will daemonize and exit, leaving the FUSE mount active.
gcsfuse --implicit-dirs \
    "$GCS_BUCKET_NAME" "$GCS_MOUNT_POINT"

# Wait for the mount to be ready. This is a simple polling check.
RETRY_COUNT=0
MAX_RETRIES=10
RETRY_DELAY_SECONDS=1
while ! mountpoint -q "$GCS_MOUNT_POINT" && [ "$RETRY_COUNT" -lt "$MAX_RETRIES" ]; do
    echo "Waiting for GCS mount ($((RETRY_COUNT+1))/$MAX_RETRIES)..."
    sleep "$RETRY_DELAY_SECONDS"
    RETRY_COUNT=$((RETRY_COUNT + 1))
done

if ! mountpoint -q "$GCS_MOUNT_POINT"; then
    echo "Error: GCS bucket mounting failed after $MAX_RETRIES retries."
    # You might want to include gcsfuse logs here if possible, though it's tricky
    # as it daemonizes. Check Cloud Logging for gcsfuse messages.
    exit 1
fi
echo "GCS bucket '$GCS_BUCKET_NAME' mounted successfully to '$GCS_MOUNT_POINT'."

# Ensure the application's specific data directory exists within the GCS mount.
# gcsfuse with --implicit-dirs should handle creation on GCS when the app first accesses it.
# Creating it here explicitly can also be done.
mkdir -p "$APP_INTERNAL_DATA_PATH"

# Graceful shutdown handler
# This function will be called when the script receives SIGTERM or SIGINT.
cleanup() {
  echo "Signal received. Unmounting GCS bucket '$GCS_MOUNT_POINT'..."
  # Attempt to unmount. Ignore errors if already unmounted or busy.
  umount "$GCS_MOUNT_POINT" || echo "Warning: Failed to unmount '$GCS_MOUNT_POINT'."
  echo "Cleanup finished."
  # The main application (./server) will also receive the signal and should shut down.
  # The script will exit after the main application exits.
}
trap cleanup TERM INT

# Run your Rust application in the foreground.
# The shell script (PID 1) will wait for it to complete.
# Your application needs to:
# 1. Listen on the port specified by the $PORT environment variable.
# 2. Use the path provided as the first argument for its data storage.
echo "Starting application server on port $PORT, with data path '$APP_INTERNAL_DATA_PATH'..."
./server "$APP_INTERNAL_DATA_PATH"

# This part is reached if the application exits normally or with an error
# (i.e., not due to a SIGTERM/SIGINT caught by the trap above).
APP_EXIT_CODE=$?
echo "Application exited with code $APP_EXIT_CODE. Performing cleanup..."
cleanup
exit $APP_EXIT_CODE
