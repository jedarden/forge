#!/usr/bin/env bash
# Worker Health Status Script
# Outputs worker health information in JSON format
# Includes paused status with timestamp and reason
#
# Usage: worker-health.sh [--all | --worker <worker_id>] [--format json|text]
#
# Options:
#   --all           Show health for all workers (default)
#   --worker <id>   Show health for specific worker
#   --format        Output format: json (default) or text

set -euo pipefail

STATUS_DIR="${FORGE_STATUS_DIR:-${HOME}/.forge/status}"
LOG_DIR="${FORGE_LOG_DIR:-${HOME}/.forge/logs}"

# Default options
OUTPUT_FORMAT="json"
WORKER_ID=""
SHOW_ALL=true

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --all)
            SHOW_ALL=true
            shift
            ;;
        --worker)
            WORKER_ID="$2"
            SHOW_ALL=false
            shift 2
            ;;
        --format)
            OUTPUT_FORMAT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: worker-health.sh [--all | --worker <worker_id>] [--format json|text]"
            echo ""
            echo "Options:"
            echo "  --all           Show health for all workers (default)"
            echo "  --worker <id>   Show health for specific worker"
            echo "  --format        Output format: json (default) or text"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# Check if status directory exists
if [[ ! -d "$STATUS_DIR" ]]; then
    if [[ "$OUTPUT_FORMAT" == "json" ]]; then
        echo '{"error": "Status directory does not exist", "workers": []}'
    else
        echo "Error: Status directory does not exist: $STATUS_DIR"
    fi
    exit 0
fi

# Function to check if PID exists
pid_exists() {
    local pid=$1
    [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null
}

# Function to check if process is zombie
is_zombie() {
    local pid=$1
    if [[ -f "/proc/$pid/stat" ]]; then
        local state
        state=$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null)
        [[ "$state" == "Z" ]]
    else
        return 1
    fi
}

# Function to get memory usage in MB
get_memory_mb() {
    local pid=$1
    if [[ -f "/proc/$pid/status" ]]; then
        local rss_kb
        rss_kb=$(grep "VmRSS:" "/proc/$pid/status" 2>/dev/null | awk '{print $2}')
        if [[ -n "$rss_kb" ]]; then
            echo $((rss_kb / 1024))
        else
            echo "0"
        fi
    else
        echo "0"
    fi
}

# Function to calculate health score
calculate_health_score() {
    local pid=$1
    local status=$2
    local last_activity=$3
    local score=100
    local issues=()

    # Check PID
    if [[ -z "$pid" ]] || ! pid_exists "$pid"; then
        score=$((score - 25))
        issues+=("dead_process")
    elif is_zombie "$pid"; then
        score=$((score - 25))
        issues+=("zombie_process")
    fi

    # Check for stale activity (> 15 minutes)
    if [[ -n "$last_activity" ]]; then
        local activity_epoch
        activity_epoch=$(date -d "$last_activity" +%s 2>/dev/null || echo 0)
        local now_epoch
        now_epoch=$(date +%s)
        local elapsed=$((now_epoch - activity_epoch))
        if [[ $elapsed -gt 900 ]]; then
            score=$((score - 25))
            issues+=("stale_activity")
        fi
    fi

    # Status-based adjustments
    case "$status" in
        "failed"|"error")
            score=$((score - 50))
            issues+=("unhealthy_status")
            ;;
        "stopped")
            score=$((score - 25))
            ;;
    esac

    # Ensure score doesn't go below 0
    [[ $score -lt 0 ]] && score=0

    echo "$score"
}

# Function to get worker health as JSON
get_worker_health_json() {
    local status_file=$1
    local worker_id
    worker_id=$(basename "$status_file" .json)

    if [[ ! -f "$status_file" ]]; then
        echo '{"worker_id": "'"$worker_id"'", "error": "Status file not found"}'
        return
    fi

    # Read status file
    local content
    content=$(cat "$status_file")

    # Parse fields using jq if available, otherwise use grep/sed
    if command -v jq &>/dev/null; then
        local status pid last_activity paused_at pause_reason
        status=$(echo "$content" | jq -r '.status // "unknown"')
        pid=$(echo "$content" | jq -r '.pid // ""')
        last_activity=$(echo "$content" | jq -r '.last_activity // ""')
        paused_at=$(echo "$content" | jq -r '.paused_at // null')
        pause_reason=$(echo "$content" | jq -r '.pause_reason // null')
        model=$(echo "$content" | jq -r '.model // ""')
        workspace=$(echo "$content" | jq -r '.workspace // ""')
        current_task=$(echo "$content" | jq -r 'if .current_task | type == "object" then .current_task.bead_id else .current_task end // null')
        tasks_completed=$(echo "$content" | jq -r '.tasks_completed // 0')
        started_at=$(echo "$content" | jq -r '.started_at // null')

        # Calculate health
        local health_score
        health_score=$(calculate_health_score "$pid" "$status" "$last_activity")

        local is_healthy="true"
        [[ $health_score -lt 50 ]] && is_healthy="false"

        local is_paused="false"
        [[ "$status" == "paused" ]] && is_paused="true"

        # Check PID health
        local pid_healthy="true"
        local pid_error=""
        if [[ -z "$pid" ]]; then
            pid_healthy="false"
            pid_error="No PID recorded"
        elif ! pid_exists "$pid"; then
            pid_healthy="false"
            pid_error="Process does not exist"
        elif is_zombie "$pid"; then
            pid_healthy="false"
            pid_error="Process is zombie"
        fi

        # Get memory usage
        local memory_mb="0"
        [[ -n "$pid" ]] && pid_exists "$pid" && memory_mb=$(get_memory_mb "$pid")

        # Build JSON output
        cat <<EOF
{
  "worker_id": "$worker_id",
  "status": "$status",
  "is_healthy": $is_healthy,
  "is_paused": $is_paused,
  "health_score": $(echo "scale=2; $health_score / 100" | bc),
  "model": $(if [[ -n "$model" ]]; then echo "\"$model\""; else echo null; fi),
  "workspace": $(if [[ -n "$workspace" ]]; then echo "\"$workspace\""; else echo null; fi),
  "pid": $(if [[ -n "$pid" ]]; then echo "$pid"; else echo null; fi),
  "pid_healthy": $pid_healthy,
  "pid_error": $(if [[ -n "$pid_error" ]]; then echo "\"$pid_error\""; else echo null; fi),
  "memory_mb": $memory_mb,
  "started_at": $(if [[ "$started_at" != "null" && -n "$started_at" ]]; then echo "\"$started_at\""; else echo null; fi),
  "last_activity": $(if [[ -n "$last_activity" ]]; then echo "\"$last_activity\""; else echo null; fi),
  "current_task": $(if [[ "$current_task" != "null" && -n "$current_task" ]]; then echo "\"$current_task\""; else echo null; fi),
  "tasks_completed": $tasks_completed,
  "paused_at": $(if [[ "$paused_at" != "null" && -n "$paused_at" ]]; then echo "\"$paused_at\""; else echo null; fi),
  "pause_reason": $(if [[ "$pause_reason" != "null" && -n "$pause_reason" ]]; then echo "\"$pause_reason\""; else echo null; fi),
  "last_checked": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
}
EOF
    else
        # Fallback without jq - basic output
        cat <<EOF
{
  "worker_id": "$worker_id",
  "error": "jq not available for parsing",
  "raw_content": $(echo "$content" | sed 's/"/\\"/g' | tr '\n' ' ')
}
EOF
    fi
}

# Function to display worker health as text
get_worker_health_text() {
    local status_file=$1
    local worker_id
    worker_id=$(basename "$status_file" .json)

    if [[ ! -f "$status_file" ]]; then
        echo "Worker: $worker_id - Status file not found"
        return
    fi

    if ! command -v jq &>/dev/null; then
        echo "Worker: $worker_id - jq required for text output"
        return
    fi

    local content
    content=$(cat "$status_file")

    local status pid last_activity paused_at pause_reason model
    status=$(echo "$content" | jq -r '.status // "unknown"')
    pid=$(echo "$content" | jq -r '.pid // "N/A"')
    last_activity=$(echo "$content" | jq -r '.last_activity // "N/A"')
    paused_at=$(echo "$content" | jq -r '.paused_at // "N/A"')
    pause_reason=$(echo "$content" | jq -r '.pause_reason // "N/A"')
    model=$(echo "$content" | jq -r '.model // "N/A"')

    local health_score
    health_score=$(calculate_health_score "$pid" "$status" "$last_activity")

    local health_indicator="[OK]"
    [[ $health_score -lt 80 ]] && health_indicator="[WARN]"
    [[ $health_score -lt 50 ]] && health_indicator="[FAIL]"

    echo "================================================"
    echo "Worker: $worker_id $health_indicator"
    echo "================================================"
    echo "  Status:        $status"
    echo "  Model:         $model"
    echo "  PID:           $pid"
    echo "  Health Score:  ${health_score}%"
    echo "  Last Activity: $last_activity"

    if [[ "$status" == "paused" ]]; then
        echo "  ---- PAUSED ----"
        echo "  Paused At:     $paused_at"
        echo "  Pause Reason:  $pause_reason"
    fi

    echo ""
}

# Main execution
main() {
    local timestamp
    timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

    if [[ "$SHOW_ALL" == "true" ]]; then
        # Get all workers
        local workers=()
        local json_outputs=()

        for status_file in "$STATUS_DIR"/*.json; do
            [[ -f "$status_file" ]] || continue

            if [[ "$OUTPUT_FORMAT" == "json" ]]; then
                json_outputs+=("$(get_worker_health_json "$status_file")")
            else
                get_worker_health_text "$status_file"
            fi
        done

        if [[ "$OUTPUT_FORMAT" == "json" ]]; then
            # Combine into array
            echo "{"
            echo "  \"timestamp\": \"$timestamp\","
            echo "  \"status_dir\": \"$STATUS_DIR\","
            echo "  \"workers\": ["

            local first=true
            for output in "${json_outputs[@]}"; do
                if [[ "$first" == "true" ]]; then
                    first=false
                else
                    echo ","
                fi
                echo "$output" | sed 's/^/    /'
            done

            echo "  ]"
            echo "}"
        fi
    else
        # Single worker
        local status_file="$STATUS_DIR/${WORKER_ID}.json"

        if [[ "$OUTPUT_FORMAT" == "json" ]]; then
            echo "{"
            echo "  \"timestamp\": \"$timestamp\","
            echo "  \"worker\": $(get_worker_health_json "$status_file")"
            echo "}"
        else
            get_worker_health_text "$status_file"
        fi
    fi
}

main
