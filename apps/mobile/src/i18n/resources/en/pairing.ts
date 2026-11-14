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
};
