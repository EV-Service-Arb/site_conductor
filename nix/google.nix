# Every Google billing account a key or credential of ours is on (docs/spec/google.md): read off its BigQuery billing export by the conductor's meters, held to `allow` by the guard (crates/guard), and the conductor's config for what it `provides`.
{
  accounts = [
    {
      name = "evinvest"; # admin@evinvest.ltd
      export = {
        table = "gen-lang-client-0759123990.billing.gcp_billing_export_v1_010027_EEDB26_C25F67";
        reader = { file = "~/.config/gcloud/accounting-reader.json"; }; # accounting-reader@gen-lang-client-0759123990
        since = "2026-10-13"; # usage before this day is not drawn
      };
      # A credit's balance is the console's reading at `read`, less what the export brought in after it.
      credits = [
        { id = "Starter:Credit-010027-EEDB26-C25F67"; name = "GCP Free Credit"; read = "2026-11-12T10:00:00Z"; left = 125.25; ends = "2027-01-13"; }
      ];
      allow = { }; # real money a month per slice, in the account's currency; a slice not named here is allowed none. Credits are always spent.
      # What its keys are for, each slot optional; secrets as `{ env = "…"; }` or `{ file = "…"; }`.
      provides = {
        places = { key = { env = "GOOGLE_MAPS_KEY_EVINVEST"; }; }; # Places, Geocoding
        browser = { key = { env = "GOOGLE_MAPS_BROWSER_KEY_EVINVEST"; }; }; # Maps JS, referrer-restricted
        # ads = { developer_token = …; client_id = …; client_secret = …; refresh_token = …; customer_id = "…"; };
      };
    }
  ];

  # Usage is drawn by the first slice whose `service` (and `sku`, where given) regexes match; by its service otherwise.
  slices = [
    { name = "video generation"; service = "^Vertex AI$"; sku = "(?i)veo|omni"; }
    { name = "LLM"; service = "^Vertex AI$|^Gemini API$"; }
    # Maps takes no spend cap: the guard holds its APIs to daily quotas, off sa_core::meters (their free units and list prices).
    {
      name = "maps";
      service = "(?i)maps|places|geocoding|directions|routes|distance matrix|street view";
      closed = [ "routes.googleapis.com" "static-maps-backend.googleapis.com" ]; # nothing of ours calls them: no request a day
    }
  ];
}
