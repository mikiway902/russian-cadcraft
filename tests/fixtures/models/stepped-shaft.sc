{
  "format": "solidcraft/63",
  "format_pending": [
    "bounding-solid-contents",
    "freeform-chain-offsets",
    "link-inherited-metadata",
    "sheet-rule-material",
    "sweep-path-across-sketches"
  ],
  "name": "Synthetic stepped hollow shaft — engineering reference",
  "units": "mm",
  "metadata": {
    "part_number": "GOST-REF-SHAFT-001",
    "designation": "ГОСТ.ТЕСТ.001",
    "purpose": "Synthetic analytic benchmark; not a manufacturing release"
  },
  "params": [],
  "features": [
    {
      "id": 1,
      "name": "Базовая шейка",
      "suppressed": false,
      "type": "cylinder",
      "base": [
        0,
        0,
        0
      ],
      "axis": [
        0,
        0,
        1
      ],
      "radius": "10",
      "height": "100",
      "operation": "new_body",
      "body_names": [
        "GOST-SHAFT"
      ]
    },
    {
      "id": 2,
      "name": "Центральная ступень",
      "suppressed": false,
      "type": "cylinder",
      "base": [
        0,
        0,
        30
      ],
      "axis": [
        0,
        0,
        1
      ],
      "radius": "15",
      "height": "40",
      "operation": "join"
    },
    {
      "id": 3,
      "name": "Сквозное осевое отверстие",
      "suppressed": false,
      "type": "cylinder",
      "base": [
        0,
        0,
        0
      ],
      "axis": [
        0,
        0,
        1
      ],
      "radius": "3",
      "height": "100",
      "operation": "cut"
    }
  ],
  "next_id": 4,
  "next_component_color_index": 0,
  "materials": {
    "GOST-SHAFT": "Synthetic reference steel"
  },
  "user_materials": [
    {
      "name": "Synthetic reference steel",
      "density": 7.85
    }
  ]
}
