# pbwatch + SwayNotificationCenter (swaync)

SwayNC supports the FDO `image-data` hint out of the box — no special configuration is needed for the widget to display. Notifications appear in the notification center with the rendered battery/ANC image.

## Recommended config

Add this to `~/.config/swaync/config.json` to fine-tune how pbwatch notifications look:

```json
{
  "notification-2fa-action": true,
  "notification-inline-replies": false,
  "fit-to-screen": true,
  "control-center-margin-top": 10,
  "control-center-margin-bottom": 10,
  "control-center-margin-right": 10,
  "control-center-margin-left": 10,

  "widgets": ["title", "dnd", "notifications"],
  "widget-config": {
    "title": {
      "text": "Notifications",
      "clear-all-button": true
    }
  }
}
```

There is no per-application image-size limit in swaync (it scales images to fit the notification width). If the widget appears too small, increase `image_size` in `~/.config/pbwatch/config.toml`.

## CSS customisation

To remove the duplicate text body (the battery percentages are already in the image), add this to `~/.config/swaync/style.css`:

```css
/* Hide text body for pbwatch notifications — image contains all info */
.notification[app-name="pbwatch"] .body {
    font-size: 0;
    margin: 0;
    padding: 0;
}
```

Then reload: `swaync-client --reload-css`

## Auto-dismiss

pbwatch sends `expire_timeout` (default 5000 ms from `auto_close` in config.toml). SwayNC respects this and dismisses the notification automatically. Set `auto_close = 0` in config.toml to keep it in the notification center until manually dismissed.
