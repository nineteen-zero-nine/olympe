#!/bin/sh
# Stand-in for akashic in tests. FAKE_AKASHIC picks a failure, and FAKE_KBS is
# what `akashic kb list --json` prints.
printf '%s\n' "$*" >> "$FAKE_AKASHIC_LOG"

case "$1" in
  init)
    if [ "$FAKE_AKASHIC" = fail-init ]; then
      echo "error: \`$4\` is already the name of /somewhere/else" >&2
      exit 2
    fi
    mkdir -p "$2/wiki" && : > "$2/akashic.toml"
    echo "created knowledge base at $2" ;;
  kb)
    printf '%s\n' "${FAKE_KBS:-[]}" ;;
  ingest)
    if [ "$FAKE_AKASHIC" = fail-ingest ]; then
      echo "error: the knowledge base has uncommitted changes" >&2
      exit 2
    fi
    echo "ingested $2" ;;
  *)
    echo "fake akashic: unexpected arguments: $*" >&2
    exit 2 ;;
esac
