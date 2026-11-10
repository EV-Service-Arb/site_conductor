# Orbita (GoLogin's Chromium) by Chrome major, for sa_browsers; the same pin as emulate_phone_cam's.
{ pkgs }:
let
  inherit (pkgs) lib;

  # One URL per major, re-uploaded in place: a hash mismatch means GoLogin re-cut it.
  tarballs = {
    "151" = pkgs.fetchurl {
      url = "https://orbita-browser-linux.gologin.com/orbita-browser-latest-151.tar.gz";
      hash = "sha256-HWPNAorAs7bNvR5h0jclHIAXvjcn2aRc4om4ievWUX4=";
    };
  };

  # Orbita is built against Debian's libcurl3-gnutls: needs soname libcurl-gnutls.so.4 exporting CURL_GNUTLS_3
  curlDebianGnutls =
    let
      curl = pkgs.curlWithGnuTls.overrideAttrs (old: {
        postPatch = (old.postPatch or "") + ''
          substituteInPlace lib/libcurl.vers.in --replace-fail '@CURL_LIBCURL_VERSIONED_SYMBOLS_SONAME@' 3
        '';
      });
    in
    pkgs.runCommand "libcurl-gnutls-debian" { nativeBuildInputs = [ pkgs.patchelf ]; } ''
      install -Dm755 ${lib.getLib curl}/lib/libcurl.so.4 $out/lib/libcurl-gnutls.so.4
      patchelf --set-soname libcurl-gnutls.so.4 $out/lib/libcurl-gnutls.so.4
    '';
in
major:
let
  unpacked = pkgs.runCommand "orbita-${major}-unpacked" { } ''
    mkdir -p $out && tar xzf ${tarballs.${major} or (throw "no Orbita pinned for Chrome ${major}")} -C $out --strip-components=1
    ls -d $out/${major}.* >/dev/null || { echo "the tarball is not Chrome ${major}" >&2; exit 1; }
  '';
  fhs = pkgs.buildFHSEnv (pkgs.appimageTools.defaultFhsEnvArgs // {
    name = "orbita";
    targetPkgs = p: (pkgs.appimageTools.defaultFhsEnvArgs.targetPkgs p) ++ [ curlDebianGnutls ];
    runScript = "${unpacked}/chrome";
  });
in
# bin/orbita, and share/locales: its UI locales
pkgs.runCommand "orbita-${major}" { } ''
  mkdir -p $out/bin $out/share
  ln -s ${fhs}/bin/orbita $out/bin/orbita
  ln -s ${unpacked}/locales $out/share/locales
''
