# Opt-in Bash prompt markers for Opterm. Source in an interactive shell.
# This changes this shell only; it never edits startup files or runs commands for you.
[[ $- == *i* ]] || return 0
[[ ${__opterm_integration_loaded-} == bash ]] && return 0
if [[ -n $(trap -p DEBUG) || -o functrace ]]; then
    printf '%s\n' 'Opterm integration was not enabled: this shell has a DEBUG trap or functrace enabled.' >&2
    return 1
fi
__opterm_integration_loaded=bash
__opterm_host=$(hostname 2>/dev/null)
__opterm_at_prompt=0
__opterm_running=0
__opterm_status=0

__opterm_encode_uri_path() {
    local value=$1 character index encoded='' LC_ALL=C
    for ((index = 0; index < ${#value}; index++)); do
        character=${value:index:1}
        case $character in
            [a-zA-Z0-9/._~-]) encoded+=$character ;;
            *) printf -v character '%%%02X' "'$character"; encoded+=$character ;;
        esac
    done
    printf '%s' "$encoded"
}
__opterm_capture_status() {
    __opterm_status=$?
    __opterm_at_prompt=0
    if (( __opterm_running )); then
        printf '\033]133;D;%s\007' "$__opterm_status"
    fi
    __opterm_running=0
    return "$__opterm_status"
}
__opterm_prompt() {
    if [[ -n $__opterm_host ]]; then
        printf '\033]7;file://%s%s\007' "$__opterm_host" "$(__opterm_encode_uri_path "$PWD")"
    fi
    # Re-wrap a prompt if a theme changed PS1 in an existing PROMPT_COMMAND.
    if [[ $PS1 != *'133;A'* ]]; then
        PS1='\[\e]133;A\a\]'"$PS1"'\[\e]133;B\a\]'
    fi
    __opterm_at_prompt=1
}
__opterm_preexec() {
    (( __opterm_at_prompt )) || return 0
    case $BASH_COMMAND in
        __opterm_capture_status*|__opterm_prompt*|__opterm_preexec*) return 0 ;;
    esac
    __opterm_at_prompt=0
    __opterm_running=1
    printf '\033]133;C\007'
}
# Retain existing prompt commands and their order. Capture the command's status
# before them, then emit metadata only once those commands have finished.
if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a"* ]]; then
    PROMPT_COMMAND=(__opterm_capture_status "${PROMPT_COMMAND[@]}" __opterm_prompt)
else
    PROMPT_COMMAND='__opterm_capture_status;'"${PROMPT_COMMAND:+$PROMPT_COMMAND;}"'__opterm_prompt'
fi
trap '__opterm_preexec' DEBUG
