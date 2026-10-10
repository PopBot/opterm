# Opt-in zsh prompt markers for Opterm. Source in an interactive shell.
[[ -o interactive ]] || return 0
[[ ${__opterm_integration_loaded-} == zsh ]] && return 0
__opterm_integration_loaded=zsh
__opterm_host=$(hostname 2>/dev/null)
__opterm_running=0
__opterm_status=0

autoload -Uz add-zsh-hook
__opterm_encode_uri_path() {
    local value=$1 character encoded='' LC_ALL=C
    integer index
    for ((index = 1; index <= ${#value}; index++)); do
        character=${value[index]}
        case $character in
            [a-zA-Z0-9/._~-]) encoded+=$character ;;
            *) printf -v character '%%%02X' "'$character"; encoded+=$character ;;
        esac
    done
    printf '%s' "$encoded"
}
__opterm_capture_status() {
    __opterm_status=$?
    if (( __opterm_running )); then
        printf '\033]133;D;%s\007' "$__opterm_status"
    fi
    __opterm_running=0
}
__opterm_precmd() {
    if [[ -n $__opterm_host ]]; then
        printf '\033]7;file://%s%s\007' "$__opterm_host" "$(__opterm_encode_uri_path "$PWD")"
    fi
    if [[ $PROMPT != *'133;A'* ]]; then
        PROMPT=$'%{\e]133;A\a%}'"$PROMPT"$'%{\e]133;B\a%}'
    fi
}
__opterm_preexec() {
    __opterm_running=1
    printf '\033]133;C\007'
}
# Add hooks rather than replace existing precmd/preexec functions.
precmd_functions=(__opterm_capture_status "${precmd_functions[@]}" __opterm_precmd)
add-zsh-hook preexec __opterm_preexec
