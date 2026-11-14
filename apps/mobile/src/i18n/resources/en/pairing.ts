export const pairing = {
  connectionMethods: {
    title: "Add connection",
    direct: {
      title: "Direct connection",
      description: "Local network or VPN.",
    },
    remoteSsh: {
      title: "Remote SSH",
      description: "Connect through the desktop SSH client.",
    },
    scanQr: {
      title: "Scan QR code",
      description: "Encrypted relay connection.",
    },
    pasteLink: {
      title: "Paste pairing link",
      description: "Encrypted relay connection.",
    },
  },
  direct: {
    title: "Direct connection",
    helper: "Enter your server address and access token.",
    fields: {
      host: "Host",
      port: "Port",
      password: "Server token",
      optional: "Required",
      useSsl: "Use SSL",
      connectionUri: "Connection URI",
    },
    advanced: {
      label: "Advanced",
      show: "Show advanced",
      hide: "Hide advanced",
    },
    passwordVisibility: {
      show: "Show password",
      hide: "Hide password",
    },
    actions: {
      cancel: "Cancel",
      connect: "Connect",
      connecting: "Connecting...",
    },
    errors: {
      hostRequired: "Host is required",
      invalidPort: "Port must be between 1 and 65535",
      invalidConnection: "Invalid connection",
      failedTitle: "Connection failed",
      failedToConnect: "We failed to connect to {{endpoint}}.",
      noAdditionalDetails: "{{detail}} (no additional details provided)",
      timedOut: "Connection timed out. Check the host/port and your network.",
      refused: "Connection refused. Is the server running at this address?",
      hostNotFound: "Host not found. Check the hostname and try again.",
      hostUnreachable: "Host is unreachable. Check your network and firewall.",
      tlsError:
        "TLS error. Direct connections use SSL only when a TLS terminator is in front of the daemon.",
      unableToConnect: "Unable to connect. Check the host/port and that the daemon is reachable.",
      details: "Details: {{detail}}",
    },
  },
  remoteSsh: {
    title: "Remote SSH",
    helper: "Connect to the remote Ait server over SSH (default server port: 7316).",
    fields: {
      target: "SSH host",
    },
    actions: {
      cancel: "Cancel",
      connect: "Connect",
      connecting: "Connecting...",
    },
    errors: {
      tokenRequired: "Enter the remote Ait server token (32–256 characters).",
      targetRequired: "SSH host is required",
      invalidTarget: "Enter a valid ssh:// host",
      failedToConnect: "Unable to connect over SSH. {{detail}}",
    },
  },
  link: {
    title: "Paste pairing link",
    helper: "Paste the pairing link from your server.",
    label: "Pairing link",
    errors: {
      required: "Paste a pairing link (.../#offer=...)",
      missingOffer: "Link must include #offer=...",
      emptyOffer: "Offer payload is empty",
      invalid: "Invalid pairing link",
      unableToPair: "Unable to pair host",
    },
    alert: {
      failedTitle: "Pairing failed",
    },
    actions: {
      cancel: "Cancel",
      pair: "Pair",
      pairing: "Pairing...",
    },
  },
  scan: {
    title: "Scan QR",
    webUnavailableTitle: "Not available on web",
    webUnavailableBody: 'QR scanning is not supported in the web build. Use "Paste link" instead.',
    backToSettings: "Back to Settings",
    cameraPermissionTitle: "Camera permission",
    cameraPermissionBody: "Allow camera access to scan the pairing QR code from your daemon.",
    grantPermission: "Grant permission",
    pairing: "Pairing...",
    unableToPair: "Unable to pair host",
    errorTitle: "Error",
  },
  device: {
    loadingOffer: "Loading pairing offer...",
    failedToLoadOffer: "Failed to load pairing offer.",
    relayDisabled: "Relay is not enabled. Enable relay to pair a device.",
    enableTitle: "Enable relay?",
    enableDescription:
      "Relay lets this device connect from anywhere. Pairing traffic is end-to-end encrypted.",
    relayDocs: "How relay works",
    relayDocsAccessibility: "Read how Ait relay works",
    enableRelay: "Enable relay",
    enablingRelay: "Enabling...",
    notNow: "Not now",
    directConnectionHint:
      "Without relay, connect directly over TCP, Tailscale, or another VPN. No QR code is created.",
    updateRequired: "Update the host to enable relay from Ait Desktop.",
    unavailable: "Pairing offer unavailable.",
    hint: "Scan this QR code with Ait on your phone, or copy the link below.",
    securityWarning:
      "Treat this pairing link like a password. Anyone with it can access this daemon.",
    qrUnavailable: "QR code unavailable.",
    qrAccessibility: "Pairing QR code",
    retry: "Retry",
    copy: "Copy",
    copied: "Copied",
  },
};
