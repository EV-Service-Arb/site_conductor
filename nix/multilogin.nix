# sa_browsers' Multilogin side: the desktop app (its launcher agent starts Mimic profiles, on
# launcher.mlx.yt:45001), run under Xvfb; and what the runner puts on a cloud phone over adb.
{ pkgs }:
let
  # upstream's `latest`, pinned by its hash: a mismatch means Multilogin cut a new one
  deb = pkgs.fetchurl {
    url = "https://cdn-mlx-prod.multiloginapp.com/desktop/latest/desktop-multilogin-ubuntu-24.04-amd64.deb";
    hash = "sha256-SbcTYblEGcCXm9uUisnYPZj7yy174pYVAa/GM7Q6Eq0=";
  };
  # every URL the app would open in a host browser lands in $MLX_OPENED (its sign-in among them)
  trap = pkgs.runCommand "mlx-open-trap" { } ''
    mkdir -p $out/bin
    for b in xdg-open x-www-browser www-browser; do
      printf '#!/bin/sh\necho "$@" >> "''${MLX_OPENED:-/dev/stderr}"\n' > $out/bin/$b
      chmod +x $out/bin/$b
    done
  '';
  # Wails on WebKitGTK 4.1; Mimic, a Chromium it downloads, runs inside the same FHS
  app = pkgs.buildFHSEnv (pkgs.appimageTools.defaultFhsEnvArgs // {
    name = "mlxapp";
    targetPkgs = p: (pkgs.appimageTools.defaultFhsEnvArgs.targetPkgs p) ++ (with p; [
      webkitgtk_4_1
      gtk3
      libsoup_3
      glib
      glib-networking
      gsettings-desktop-schemas
      cacert
      libayatana-appindicator
      libpulseaudio
      ffmpeg
      libdrm
      mesa
      libGL
      vulkan-loader
    ]) ++ (with p.gst_all_1; [ gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav ]);
    profile = ''
      export PATH=${trap}/bin:$PATH
      export GST_PLUGIN_SYSTEM_PATH_1_0=/usr/lib/gstreamer-1.0
      export GIO_MODULE_DIR=/usr/lib/gio/modules
    '';
    runScript = "${pkgs.runCommand "mlxapp-unpacked" { nativeBuildInputs = [ pkgs.dpkg ]; } "dpkg-deb -x ${deb} $out"}/opt/mlxapp/desktop.bin";
  });
in
{
  # the app on its own display: BROWSERS_MLX
  mlx = pkgs.writeShellApplication {
    name = "mlx-headless";
    runtimeInputs = [ pkgs.xvfb-run ];
    # Wayland first otherwise: GTK and Mimic would open on the host's session, not Xvfb
    text = ''
      unset WAYLAND_DISPLAY
      export GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
      exec xvfb-run -a -s "-screen 0 1280x800x24" ${app}/bin/mlxapp "$@"
    '';
  };
  # what the runner drives a cloud phone with: BROWSERS_ADB, BROWSERS_BUSYBOX, BROWSERS_ADB_KEYBOARD
  phoneTools = pkgs.linkFarm "phone-tools" {
    adb = "${pkgs.android-tools}/bin/adb";
    # the exit read from inside the phone: its own wget, the phone has none
    busybox = "${pkgs.pkgsCross.aarch64-multiplatform.pkgsStatic.busybox}/bin/busybox";
    # text past ASCII (`input text` types none): ADB_INPUT_B64 broadcasts
    "ADBKeyboard.apk" = pkgs.fetchurl {
      url = "https://raw.githubusercontent.com/senzhk/ADBKeyBoard/d7b27288bc8c1c8348a79ea6896bd7352c30a9fc/ADBKeyboard.apk";
      hash = "sha256-5pit6lYzE1oGewOPmgz0G6pN4JiIcTqBWT+yuWgs3Fk=";
    };
  };
}
