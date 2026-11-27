#!/bin/sh
# Stand-in for Claude Code in tests. olympe runs it as
# `claude <flags...> <prompt>` for a session in the terminal: the lead, in the
# project's folder, and the designer, in the folder of the process. It runs it
# as `claude -p <prompt> <flags...>` for its own runs: in the folder of the
# process for `design --auto` and `improve`, in the project's folder for the
# retrospective.
printf '=== call in %s\n%s\n' "$(pwd -P)" "$*" >> "$FAKE_LOG"

reply() {
  printf '{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"%s","total_cost_usd":0.05,"structured_output":%s}\n' "$1" "$2"
}

specialist() {
  mkdir -p specialists
  cat > "specialists/$1.md" <<EOF
---
description: The $1 of the fake team.
---
You are the $1.
EOF
}

# What the designer writes for a new process: FAKE_DESIGN picks it.
design() {
  process=$(basename "$(pwd -P)")
  if [ "$FAKE_DESIGN" != nothing ]; then
    printf '# %s\n\n## Goal\nCatch defects before merge.\n' "$process" > brief.md
    cat > process.md <<EOF
---
description: A process written by the fake designer.
---
First the \`reader\`, then the \`tester\`.
EOF
    specialist reader
    specialist tester
  fi
  case "$FAKE_DESIGN" in
    broken|repair) echo "no front matter" > specialists/tester.md ;;
    stray)
      root=$(printf '%s\n' "$1" | sed -n 's/^The project `[^`]*` is in `\([^`]*\)`.*/\1/p')
      echo stray > "$root/stray.txt" ;;
    skill|unused-skill)
      mkdir -p skills/checklist
      cat > skills/checklist/SKILL.md <<EOF
---
name: checklist
description: The checks a change must pass. Use before approving a change.
---
1. Run the suite.
EOF
      [ "$FAKE_DESIGN" = skill ] && echo "The \`tester\` uses \`$process:checklist\`." >> process.md ;;
  esac
}

# A session in the terminal.
if [ "$1" != "-p" ]; then
  for prompt; do :; done
  case "$prompt" in
    # The designer, on a process that does not exist yet.
    "Design the "*) design "$prompt" ;;
    # The designer, on a process that exists: FAKE_REVISE picks what it does.
    "Revise the "*)
      case "$FAKE_REVISE" in
        nothing) ;;
        drop-tester) rm specialists/tester.md ;;
        repair) specialist tester ;;
        *) echo "REVISED." >> process.md ;;
      esac ;;
  esac
  # The lead session does nothing. FAKE_EXIT is how a session ends.
  exit "${FAKE_EXIT:-0}"
fi

case "$2" in
  # `olympe design --auto`, on a process that does not exist yet.
  "Design the "*)
    design "$2"
    reply fake-design '{"summary":"A reader and a tester."}' ;;

  # `olympe design --auto`, on a process that exists.
  "Change the "*)
    echo "CHANGED." >> process.md
    reply fake-design '{"changed":true,"summary":"The playbook has a new phase."}' ;;

  "olympe could not load "*)
    [ "$FAKE_DESIGN" = repair ] && specialist tester
    reply fake-design '{"summary":"A reader and a tester."}' ;;

  # What olympe's check found is left as it is.
  "olympe checked the team "*)
    reply fake-design '{"changed":false,"summary":"Nothing to correct."}' ;;

  # `olympe improve`: FAKE_IMPROVE picks what the designer does.
  "Improve the "*)
    case "$FAKE_IMPROVE" in
      unchanged) reply fake-improve '{"changed":false,"summary":"One unusual run explains the feedback."}' ;;
      breaks)
        echo "no front matter any more" > process.md
        reply fake-improve '{"changed":true,"summary":"Rewrote the playbook."}' ;;
      brief)
        echo "Done means whatever is quick." >> brief.md
        echo "IMPROVED." >> process.md
        reply fake-improve '{"changed":true,"summary":"Loosened the brief."}' ;;
      *)
        echo "IMPROVED." >> process.md
        reply fake-improve '{"changed":true,"summary":"The playbook now names the test command."}' ;;
    esac ;;

  # The retrospective: FAKE_RETRO picks what the lead writes, in the folder
  # of the run, which olympe has created.
  "The session is over"*)
    dir=$(printf '%s\n' "$2" | sed -n 's/^Write up to two files in `\([^`]*\)\/`.*/\1/p')
    case "$FAKE_RETRO" in
      none) ;;
      fail)
        printf '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"No conversation found"}\n'
        exit 1 ;;
      learnings) echo "## The cache is per user" > "$dir/learnings.md" || exit 3 ;;
      feedback) echo "## reader: read the tests too" > "$dir/feedback.md" || exit 3 ;;
      *)
        echo "## The cache is per user" > "$dir/learnings.md" || exit 3
        echo "## reader: read the tests too" > "$dir/feedback.md" || exit 3 ;;
    esac
    reply fake-retro null ;;

  *)
    echo "fake claude: unexpected prompt: $2" >&2
    exit 2 ;;
esac
