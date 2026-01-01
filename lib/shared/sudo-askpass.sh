#!/bin/bash
# This script prompts for a password using osascript and outputs it to stdout
# Used by sudo --askpass to get the password

osascript -e 'display dialog "balenaEtcher needs privileged access in order to flash disks.\n\nType your password to allow this." default answer "" with hidden answer buttons {"Cancel", "Ok"} default button "Ok" with icon caution' -e 'text returned of result' 2>/dev/null
