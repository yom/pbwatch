# References

## Google Fast Pair Specification (GFPS) — Message Stream

The RFCOMM-based protocol used to read battery state and ANC mode from Pixel Buds.

- Spec overview: https://developers.google.com/nearby/fast-pair/specifications/introduction
- Message Stream protocol (group/code table, battery encoding, ANC codes):
  https://developers.google.com/nearby/fast-pair/specifications/extensions/messagestream

Key message types used by this project:

| Group | Code | Name | Notes |
|-------|------|------|-------|
| `0x03` | `0x01` | ModelId | 3-byte big-endian product ID |
| `0x03` | `0x03` | BatteryInfo | 3 bytes: `[left, right, case]`; bit7=charging, bits6–0=percentage, `0x7f`=not present |
| `0x03` | `0x09` | FirmwareVersion | UTF-8 string |
| `0x08` | `0x11` | GetState | Request sent on connect to trigger ANC state reply |
| `0x08` | `0x13` | ActiveNoiseCancellationState | ANC mode + supported modes bitmask |

Battery byte encoding: `bit7` = charging flag, `bits6–0` = percentage (`0x7f` = component not present, reported as `-1`).

## qzed/pbpctrl — Rust reference implementation

Open-source Rust implementation of the Pixel Buds protocol, used to confirm byte order and message structure.

- Repository: https://github.com/qzed/pbpctrl
- Relevant source: `libgfps/src/msg/types.rs` — message type definitions and byte-order constants confirming `data[0]=left, data[1]=right, data[2]=case` for `BatteryInfo`

## BlueZ D-Bus API — `org.bluez.Device1`

Used by `pbwatch` for device state queries without spawning subprocesses.

- Reference: https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/device-api.txt

Properties used:

| Property | Access | Description |
|----------|--------|-------------|
| `Connected` | read-only | `true` when the device has an active Bluetooth connection |
| `Name` | read-only | The device's own Bluetooth name as broadcast (EIR Local Name) |
| `Alias` | read-write | Local label override; defaults to `Name` when unset. Set with `bluetoothctl set-alias` |
| `Modalias` | read-only | `bluetooth:vVENDORpPRODUCTdREVISION`; the `p` field encodes the Fast Pair Model ID |

Object path pattern: `/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF`

## Google Fast Pair Device Catalog

Registry mapping Fast Pair Model IDs to product names (e.g. `0x003004` → "Pixel Buds Pro"). Numeric only — no human-readable model string is transmitted by the device itself.

- https://developers.google.com/nearby/fast-pair/device-catalog
