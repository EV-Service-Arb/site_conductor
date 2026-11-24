# Every Google billing account a key or credential of ours is on (docs/spec/google.md): read off its BigQuery billing export by the conductor's meters, held by the guard (crates/guard) to what an admin sets it to in the meters card, and the conductor's config for what it `provides`.
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
        { id = "Starter:Credit-010027-EEDB26-C25F67"; name = "GCP Free Credit"; read = "2026-11-23T19:55:00Z"; left = 100.94; ends = "2027-01-13"; }
      ];
      # What its keys are for, each slot optional; secrets as `{ env = "…"; }` or `{ file = "…"; }`.
      provides = {
        places = { key = { env = "GOOGLE_MAPS_KEY_EVINVEST"; }; }; # Places, Geocoding
        browser = { key = { env = "GOOGLE_MAPS_BROWSER_KEY_EVINVEST"; }; }; # Maps JS, referrer-restricted
        # ads = { developer_token = …; client_id = …; client_secret = …; refresh_token = …; customer_id = "…"; };
      };
    }
    {
      name = "v7916"; # v79166789533@gmail.com
      export = null; # its billing account 01AFF0-385455-2AC92F is closed: nothing of ours uses it. Once open: gmp-demo-project-985008120.billing.gcp_billing_export_v1_01AFF0_385455_2AC92F, its reader ~/.config/gcloud/v7916-reader.json
      credits = [ ];
      provides.places = { key = { env = "GOOGLE_MAPS_KEY"; }; }; # Maps Platform Demo API Key: Places, Geocoding, Maps JS, Routes, YouTube
    }
    {
      name = "icos1"; # 1cryptoicos@gmail.com
      export = null; # its billing account is not open yet: nothing of ours uses it. Its reader, once it is: ~/.config/gcloud/icos1-reader.json (accounting-reader@gmp-demo-project-232535659)
      credits = [ ];
      provides.places = { key = { env = "GOOGLE_MAPS_KEY_ICOS1"; }; }; # Maps Platform Demo API Key: Places, Geocoding, Maps JS, Routes
    }
  ];

  # Usage is drawn by the first slice whose `service` (and `sku`, where given) regexes match; by its service otherwise.
  slices = [
    { name = "video generation"; service = "^Vertex AI$"; sku = "(?i)veo|omni"; }
    { name = "LLM"; service = "^Vertex AI$|^Gemini API$"; }
    # Maps takes no spend cap: the guard holds its metered APIs to daily quotas, off sa_core::meters (their free units and list prices), and every other one with daily quotas at none.
    { name = "maps"; service = "(?i)maps|places|geocoding|directions|routes|distance matrix|street view"; }
  ];
}
