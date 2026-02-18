#!/bin/bash

# ask_user_popup.sh
# Display a question in a tmux popup and capture user response
#
# Usage (heredoc):
#   bash ask_user_popup.sh <<'EOF'
#   Question text here.
#   Multiple lines are fine.
#   ---
#   Option A description
#   Option B description
#   Option C description
#   EOF
#
# Format: Lines before "---" are the question. Lines after are options.
# Special option: If user enters "e", opens $EDITOR to write a free-form response.

set -e

# Read input from stdin (heredoc)
INPUT=$(cat)

# Split on "---" separator
QUESTION=$(echo "$INPUT" | sed '/^---$/,$d')
OPTIONS_RAW=$(echo "$INPUT" | sed '1,/^---$/d')

if [ -z "$QUESTION" ] || [ -z "$OPTIONS_RAW" ]; then
  echo "Error: Input must contain question, '---' separator, and options." >&2
  echo "Usage: bash $0 <<'EOF'" >&2
  echo "Question text" >&2
  echo "---" >&2
  echo "Option 1" >&2
  echo "Option 2" >&2
  echo "EOF" >&2
  exit 1
fi

# Parse options into array
OPTIONS=()
while IFS= read -r line; do
  [ -n "$line" ] && OPTIONS+=("$line")
done <<< "$OPTIONS_RAW"

if [ ${#OPTIONS[@]} -lt 2 ]; then
  echo "Error: At least 2 options are required." >&2
  exit 1
fi

# Create temporary files
RESPONSE_FILE=$(mktemp)
SCRIPT_FILE=$(mktemp)
QUESTION_FILE=$(mktemp)
echo "$QUESTION" > "$QUESTION_FILE"
trap "rm -f $RESPONSE_FILE $SCRIPT_FILE $QUESTION_FILE" EXIT

# Find the user's main tmux session (non-claude session)
TARGET_SESSION=$(tmux list-sessions -F '#{session_name}' 2>/dev/null | grep -v '^claude' | head -1)
if [ -z "$TARGET_SESSION" ]; then
  TARGET_SESSION=$(tmux list-sessions -F '#{session_name}' 2>/dev/null | head -1)
fi

# Create the popup script
cat > "$SCRIPT_FILE" << 'POPUP_SCRIPT'
#!/bin/bash
RESPONSE_FILE="$1"
QUESTION_FILE="$2"
shift 2
OPTIONS=("$@")
EDITOR="${EDITOR:-vi}"

cat "$QUESTION_FILE"
echo ""
for i in "${!OPTIONS[@]}"; do
  echo "  $((i + 1)). ${OPTIONS[$i]}"
done
echo ""
echo "  e. Open editor (free-form response)"
echo ""

while true; do
  read -p "Choice (1-${#OPTIONS[@]} or e): " choice
  if [ "$choice" = "e" ] || [ "$choice" = "E" ]; then
    EDIT_FILE=$(mktemp)
    cat > "$EDIT_FILE" << TEMPLATE
# Write your response below this line. Lines starting with # are ignored.
# Question was:
$(sed 's/^/# /' "$QUESTION_FILE")
#
# Options were:
$(for i in "${!OPTIONS[@]}"; do echo "#   $((i + 1)). ${OPTIONS[$i]}"; done)

TEMPLATE
    $EDITOR "$EDIT_FILE"
    grep -v '^#' "$EDIT_FILE" | sed '/^$/d' > "$RESPONSE_FILE"
    rm -f "$EDIT_FILE"
    if [ -s "$RESPONSE_FILE" ]; then
      break
    fi
    echo "Empty response. Please try again."
    continue
  fi
  if [[ "$choice" =~ ^[0-9]+$ ]] && [ "$choice" -ge 1 ] && [ "$choice" -le ${#OPTIONS[@]} ]; then
    echo "${OPTIONS[$((choice - 1))]}" > "$RESPONSE_FILE"
    break
  fi
  echo "Invalid choice. Please try again."
done
POPUP_SCRIPT

chmod +x "$SCRIPT_FILE"

# Build the command with properly quoted options
CMD="bash '$SCRIPT_FILE' '$RESPONSE_FILE' '$QUESTION_FILE'"
for opt in "${OPTIONS[@]}"; do
  CMD="$CMD '${opt//\'/\'\\\'\'}'"
done

# Calculate popup size based on content
QUESTION_LINES=$(echo "$QUESTION" | wc -l)
POPUP_HEIGHT=$((QUESTION_LINES + ${#OPTIONS[@]} + 8))
if [ "$POPUP_HEIGHT" -lt 12 ]; then
  POPUP_HEIGHT=12
fi
if [ "$POPUP_HEIGHT" -gt 40 ]; then
  POPUP_HEIGHT=40
fi

# Display the popup
if [ -n "$TMUX" ] && [ -n "$TARGET_SESSION" ]; then
  tmux display-popup -t "$TARGET_SESSION" -w 80 -h "$POPUP_HEIGHT" -E "$CMD"
else
  bash "$SCRIPT_FILE" "$RESPONSE_FILE" "$QUESTION_FILE" "${OPTIONS[@]}"
fi

# Output the response
if [ -f "$RESPONSE_FILE" ] && [ -s "$RESPONSE_FILE" ]; then
  cat "$RESPONSE_FILE"
else
  echo "No response"
  exit 1
fi
