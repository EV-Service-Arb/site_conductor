# google.nix's accounts, and its slices, with every secret slot (`{ env }` or `{ file }`) named as `{ env = "<prefix>_<ACCOUNT>_<SLOT>"; }`,
# for a deploy that supplies each from its own store; `vars` maps each variable to its `<account>_<slot>` id.
# A slot is its path less `export`, `provides` and a trailing `key`: `reader`, `places`, `browser`, `ads_client_id`.
prefix:
let
  inherit (builtins) attrNames concatMap concatStringsSep filter genList isAttrs listToAttrs mapAttrs replaceStrings stringLength substring;
  letters = s: genList (i: substring i 1 s) (stringLength s);
  upper = replaceStrings (letters "abcdefghijklmnopqrstuvwxyz-") (letters "ABCDEFGHIJKLMNOPQRSTUVWXYZ_");
  isSlot = v: isAttrs v && (attrNames v == [ "env" ] || attrNames v == [ "file" ]);
  id = account: path: concatStringsSep "_" ([ account.name ] ++ filter (p: p != "export" && p != "provides" && p != "key") path);
  var = account: path: "${prefix}_${upper (id account path)}";
  rewrite = account: path: v:
    if isSlot v then { env = var account path; }
    else if isAttrs v then mapAttrs (k: rewrite account (path ++ [ k ])) v
    else v;
  slots = account: path: v:
    if isSlot v then [{ name = var account path; value = id account path; }]
    else if isAttrs v then concatMap (k: slots account (path ++ [ k ]) v.${k}) (attrNames v)
    else [ ];
  inherit (import ./google.nix) accounts slices;
in
{
  inherit slices;
  accounts = map (a: rewrite a [ ] a) accounts;
  vars = listToAttrs (concatMap (a: slots a [ ] a) accounts);
}
