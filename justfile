set positional-arguments := true

default:
  #!/usr/bin/env bash
  set -euo pipefail
  command=$(
    just --unsorted --summary |
    sed 's# #\n#g' |
    grep -v default |
    fzf \
      --layout reverse \
      --preview 'echo && just --show {} | bat -f --language=bash --style=numbers' \
      --preview-window 'right:50%,border-none'
  )
  just put 'bold,setaf 6' "just $command"
  just "$command"

server:
	cargo run --

build:
	cargo build

# See https://www.mankier.com/5/terminfo#Description-Predefined_Capabilities for a list of capabilities.
put caps *args:
  #!/usr/bin/env bash
  set -euo pipefail
  readarray -td, caps < <(printf "%s" "$1")
  shift
  escape_sequence=''
  for c in "${caps[@]}"; do
    # shellcheck disable=SC2046 # for c='setaf 4' for example
    escape_sequence+=$(tput $c)
  done
  if [[ -t 1 ]]; then
    printf "%s%s%s\n" "$escape_sequence" "$*" "$(tput sgr0)"
  else
    printf "%s\n" "$*"
  fi
  

