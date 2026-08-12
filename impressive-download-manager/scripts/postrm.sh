#!/bin/sh
set -e

# Impressive Download Manager post-uninstall cleanup script
# Removes user-level autostart .desktop entries and browser native messaging host manifests
# to prevent ghost launcher icons (e.g. Deepin, GNOME, KDE) upon package removal.
# Works across standard distros (/home/*, /root) and OSTree/immutable distros (/var/home/*).
# Explicitly DOES NOT remove user download files or unrelated user configurations.

clean_user_dir() {
    user_home="$1"
    if [ -d "$user_home" ]; then
        # 1. Remove leftover autostart desktop entry
        rm -f "$user_home/.config/autostart/impressive-download-manager.desktop"
        rm -f "$user_home/.config/autostart/com.shaheer.impressive-download-manager.desktop"

        # 2. Remove leftover browser native messaging host manifests
        rm -f "$user_home/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/com.impressive.idm.json"
        rm -f "$user_home/.config/google-chrome/NativeMessagingHosts/com.impressive.idm.json"
        rm -f "$user_home/.config/chromium/NativeMessagingHosts/com.impressive.idm.json"
        rm -f "$user_home/.config/microsoft-edge/NativeMessagingHosts/com.impressive.idm.json"
        rm -f "$user_home/.config/opera/NativeMessagingHosts/com.impressive.idm.json"
        rm -f "$user_home/.config/vivaldi/NativeMessagingHosts/com.impressive.idm.json"
        rm -f "$user_home/.mozilla/native-messaging-hosts/com.impressive.idm.json"
    fi
}

# Scan /home/*, /var/home/* (OSTree / Fedora Silverblue / SteamOS), and /root
for dir in /home/* /var/home/* /root; do
    if [ -d "$dir" ] && [ ! -L "$dir" ]; then
        clean_user_dir "$dir"
    fi
done

# Update system desktop database and icon caches if available
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
fi

exit 0
