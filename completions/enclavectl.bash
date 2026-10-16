# bash completion for enclavectl

_enclavectl() {
    local cur prev words cword
    # Keep a MAC address after -a in one word: bash would otherwise split it at the colons.
    _init_completion -n : || return

    case $prev in
        -a | --address | -c | --channel)
            return
            ;;
    esac

    if [[ $cur == -* ]]; then
        COMPREPLY=($(compgen -W '-a --address -c --channel --json -q --quiet -h --help' -- "$cur"))
        return
    fi

    # Find the command and how many arguments follow it, skipping options and their values.
    local i cmd='' nargs=0
    for ((i = 1; i < cword; i++)); do
        case ${words[i]} in
            -a | --address | -c | --channel) ((i++)) ;;
            -[!0-9]*) ;;
            *)
                if [[ -z $cmd ]]; then
                    cmd=${words[i]}
                else
                    ((nargs++))
                fi
                ;;
        esac
    done

    local choices=''
    if [[ -z $cmd ]]; then
        choices='status on off mute volume input cec raw'
    elif ((nargs == 0)); then
        case $cmd in
            mute) choices='on off' ;;
            input) choices='hdmi1 hdmi2 hdmi3 optical arc bt' ;;
            cec) choices='off on arc' ;;
            raw) choices=$(echo {144..159}) ;;
        esac
    fi
    COMPREPLY=($(compgen -W "$choices" -- "$cur"))
}

complete -F _enclavectl enclavectl
