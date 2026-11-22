{
  "format": "solidcraft/63",
  "format_pending": ["bounding-solid-contents", "freeform-chain-offsets", "link-inherited-metadata", "sheet-rule-material", "sweep-path-across-sketches"],
  "name": "Off-centre imposed section world-coordinate proof",
  "units": "mm",
  "params": [],
  "features": [
    {"id": 1, "name": "Wide base", "suppressed": false, "type": "box", "corner": [0, 0, 0], "length": "100", "width": "20", "height": "10", "operation": "new_body", "body_names": ["STEPPED-SOLID"]},
    {"id": 2, "name": "Off-centre neck", "suppressed": false, "type": "box", "corner": [30, 0, 9], "length": "20", "width": "20", "height": "21", "operation": "join"}
  ],
  "next_id": 3,
  "next_component_color_index": 0
}
