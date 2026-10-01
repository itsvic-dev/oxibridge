self:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.oxibridge;
  package = self.packages.${pkgs.system}.default;
  settingsFormat = pkgs.formats.yaml { };
  settingsFile = settingsFormat.generate "oxibridge.yml" cfg.settings;
  secretNames = lib.imap0 (i: _: "secret-${toString i}") cfg.secretFiles;
in {
  imports = [
    (lib.mkChangedOptionModule [ "services" "oxibridge" "configFile" ] [
      "services"
      "oxibridge"
      "secretFiles"
    ] (config: [ config.services.oxibridge.configFile ]))
  ];

  options = {
    services.oxibridge = {
      enable = lib.mkEnableOption
        "Oxibridge, a bot connecting multiple Telegram groups and Discord channels";
      settings = lib.mkOption {
        type = settingsFormat.type;
        default = { };
        description = ''
          Configuration for Oxibridge. See `config.example.yml` for the options.
          This is written to the Nix store, so put secrets in `secretFiles` instead.
        '';
      };
      secretFiles = lib.mkOption {
        type = lib.types.listOf lib.types.path;
        default = [ ];
        description = ''
          YAML files that are deep-merged over `settings`, in order.
          They are passed with `LoadCredential`, so they can be outside the Nix store.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.oxibridge = {
      after = [ "network.target" "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = lib.getExe package;
        Restart = "on-failure";
        DynamicUser = true;

        LoadCredential =
          lib.zipListsWith (name: path: "${name}:${path}") secretNames
          cfg.secretFiles;

        NoNewPrivileges = true;
        RemoveIPC = true;
        PrivateTmp = true;
        ProcSubset = "pid";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "full";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        UMask = "0077";
      };

      environment.CONFIG_FILE = lib.concatStringsSep ":"
        ([ "${settingsFile}" ] ++ map (name: "%d/${name}") secretNames);
    };
  };
}
