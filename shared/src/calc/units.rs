//! 15-dimension physical unit conversion engine.
//!
//! Provides unit definitions with affine offsets, linear multipliers,
//! and reciprocal conversions across 14 physical dimensions (with currency
//! handled by `currency.rs`). Parses natural language queries such as
//! `100 miles in km` or `32 F to C`.

use crate::calc::CalcError;
use std::f64::consts::PI;

/// Physical dimensions supported by the conversion engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    Angle,
    Area,
    DataStorage,
    Energy,
    Force,
    FuelEconomy,
    Length,
    Mass,
    Power,
    Pressure,
    Speed,
    Temperature,
    Time,
    Volume,
}

/// A physical unit specification with base conversion factor and affine offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitDef {
    /// Canonical unit symbol (e.g. `"km"`, `"F"`).
    pub symbol: &'static str,
    /// Canonical human-readable name (e.g. `"kilometer"`).
    pub name: &'static str,
    /// Physical dimension category.
    pub dimension: Dimension,
    /// Multiplier to convert unit to base dimension unit.
    pub multiplier: f64,
    /// Affine offset added after multiplying when converting to base unit.
    pub offset: f64,
    /// True if unit conversion is reciprocal (e.g. L/100km).
    pub is_reciprocal: bool,
    /// Recognized aliases / symbols.
    pub aliases: &'static [&'static str],
}

const LENGTH_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "m",
        name: "meter",
        dimension: Dimension::Length,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["m", "meter", "meters", "metre", "metres"],
    },
    UnitDef {
        symbol: "km",
        name: "kilometer",
        dimension: Dimension::Length,
        multiplier: 1000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["km", "kilometer", "kilometers", "kilometre", "kilometres"],
    },
    UnitDef {
        symbol: "cm",
        name: "centimeter",
        dimension: Dimension::Length,
        multiplier: 0.01,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "cm",
            "centimeter",
            "centimeters",
            "centimetre",
            "centimetres",
        ],
    },
    UnitDef {
        symbol: "mm",
        name: "millimeter",
        dimension: Dimension::Length,
        multiplier: 0.001,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "mm",
            "millimeter",
            "millimeters",
            "millimetre",
            "millimetres",
        ],
    },
    UnitDef {
        symbol: "um",
        name: "micrometer",
        dimension: Dimension::Length,
        multiplier: 1e-6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["um", "µm", "micrometer", "micrometers"],
    },
    UnitDef {
        symbol: "nm",
        name: "nanometer",
        dimension: Dimension::Length,
        multiplier: 1e-9,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["nm", "nanometer", "nanometers"],
    },
    UnitDef {
        symbol: "in",
        name: "inch",
        dimension: Dimension::Length,
        multiplier: 0.0254,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["in", "inch", "inches", "\""],
    },
    UnitDef {
        symbol: "ft",
        name: "foot",
        dimension: Dimension::Length,
        multiplier: 0.3048,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ft", "foot", "feet", "'"],
    },
    UnitDef {
        symbol: "yd",
        name: "yard",
        dimension: Dimension::Length,
        multiplier: 0.9144,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["yd", "yard", "yards"],
    },
    UnitDef {
        symbol: "mi",
        name: "mile",
        dimension: Dimension::Length,
        multiplier: 1609.344,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mi", "mile", "miles"],
    },
    UnitDef {
        symbol: "nmi",
        name: "nautical mile",
        dimension: Dimension::Length,
        multiplier: 1852.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["nmi", "nautical mile", "nautical miles"],
    },
];

const MASS_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "kg",
        name: "kilogram",
        dimension: Dimension::Mass,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kg", "kilogram", "kilograms"],
    },
    UnitDef {
        symbol: "g",
        name: "gram",
        dimension: Dimension::Mass,
        multiplier: 0.001,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["g", "gram", "grams"],
    },
    UnitDef {
        symbol: "mg",
        name: "milligram",
        dimension: Dimension::Mass,
        multiplier: 1e-6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mg", "milligram", "milligrams"],
    },
    UnitDef {
        symbol: "ug",
        name: "microgram",
        dimension: Dimension::Mass,
        multiplier: 1e-9,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ug", "µg", "microgram", "micrograms"],
    },
    UnitDef {
        symbol: "oz",
        name: "ounce",
        dimension: Dimension::Mass,
        multiplier: 0.028_349_523_125,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["oz", "ounce", "ounces"],
    },
    UnitDef {
        symbol: "lb",
        name: "pound",
        dimension: Dimension::Mass,
        multiplier: 0.453_592_37,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["lb", "lbs", "pound", "pounds"],
    },
    UnitDef {
        symbol: "st",
        name: "stone",
        dimension: Dimension::Mass,
        multiplier: 6.350_293_18,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["st", "stone", "stones"],
    },
    UnitDef {
        symbol: "tonne",
        name: "metric ton",
        dimension: Dimension::Mass,
        multiplier: 1000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["t", "tonne", "tonnes", "metric ton", "metric tons"],
    },
    UnitDef {
        symbol: "ton",
        name: "short ton",
        dimension: Dimension::Mass,
        multiplier: 907.184_74,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ton", "tons", "short ton", "short tons"],
    },
];

const TEMPERATURE_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "C",
        name: "Celsius",
        dimension: Dimension::Temperature,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["c", "°c", "celsius", "centigrade"],
    },
    UnitDef {
        symbol: "K",
        name: "Kelvin",
        dimension: Dimension::Temperature,
        multiplier: 1.0,
        offset: -273.15,
        is_reciprocal: false,
        aliases: &["k", "kelvin"],
    },
    UnitDef {
        symbol: "F",
        name: "Fahrenheit",
        dimension: Dimension::Temperature,
        multiplier: 5.0 / 9.0,
        offset: -32.0 * (5.0 / 9.0),
        is_reciprocal: false,
        aliases: &["f", "°f", "fahrenheit"],
    },
];

const DATA_STORAGE_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "b",
        name: "bit",
        dimension: Dimension::DataStorage,
        multiplier: 0.125,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["b", "bit", "bits"],
    },
    UnitDef {
        symbol: "B",
        name: "byte",
        dimension: Dimension::DataStorage,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["B", "byte", "bytes"],
    },
    UnitDef {
        symbol: "KB",
        name: "kilobyte",
        dimension: Dimension::DataStorage,
        multiplier: 1e3,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kb", "kilobyte", "kilobytes"],
    },
    UnitDef {
        symbol: "MB",
        name: "megabyte",
        dimension: Dimension::DataStorage,
        multiplier: 1e6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mb", "megabyte", "megabytes"],
    },
    UnitDef {
        symbol: "GB",
        name: "gigabyte",
        dimension: Dimension::DataStorage,
        multiplier: 1e9,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["gb", "gigabyte", "gigabytes"],
    },
    UnitDef {
        symbol: "TB",
        name: "terabyte",
        dimension: Dimension::DataStorage,
        multiplier: 1e12,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["tb", "terabyte", "terabytes"],
    },
    UnitDef {
        symbol: "PB",
        name: "petabyte",
        dimension: Dimension::DataStorage,
        multiplier: 1e15,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["pb", "petabyte", "petabytes"],
    },
    UnitDef {
        symbol: "KiB",
        name: "kibibyte",
        dimension: Dimension::DataStorage,
        multiplier: 1024.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kib", "kibibyte", "kibibytes"],
    },
    UnitDef {
        symbol: "MiB",
        name: "mebibyte",
        dimension: Dimension::DataStorage,
        multiplier: 1024.0 * 1024.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mib", "mebibyte", "mebibytes"],
    },
    UnitDef {
        symbol: "GiB",
        name: "gibibyte",
        dimension: Dimension::DataStorage,
        multiplier: 1024.0 * 1024.0 * 1024.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["gib", "gibibyte", "gibibytes"],
    },
    UnitDef {
        symbol: "TiB",
        name: "tebibyte",
        dimension: Dimension::DataStorage,
        multiplier: 1_099_511_627_776.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["tib", "tebibyte", "tebibytes"],
    },
];

const SPEED_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "m/s",
        name: "meter per second",
        dimension: Dimension::Speed,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["m/s", "mps", "meter per second", "meters per second"],
    },
    UnitDef {
        symbol: "km/h",
        name: "kilometer per hour",
        dimension: Dimension::Speed,
        multiplier: 1.0 / 3.6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "km/h",
            "kph",
            "kmh",
            "kilometer per hour",
            "kilometers per hour",
        ],
    },
    UnitDef {
        symbol: "mph",
        name: "mile per hour",
        dimension: Dimension::Speed,
        multiplier: 0.44704,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mph", "mile per hour", "miles per hour"],
    },
    UnitDef {
        symbol: "knot",
        name: "knot",
        dimension: Dimension::Speed,
        multiplier: 1852.0 / 3600.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["knot", "knots", "kn"],
    },
    UnitDef {
        symbol: "ft/s",
        name: "foot per second",
        dimension: Dimension::Speed,
        multiplier: 0.3048,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ft/s", "fps", "foot per second", "feet per second"],
    },
];

const ANGLE_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "rad",
        name: "radian",
        dimension: Dimension::Angle,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["rad", "radian", "radians"],
    },
    UnitDef {
        symbol: "deg",
        name: "degree",
        dimension: Dimension::Angle,
        multiplier: PI / 180.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["deg", "degree", "degrees", "°"],
    },
    UnitDef {
        symbol: "grad",
        name: "gradian",
        dimension: Dimension::Angle,
        multiplier: PI / 200.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["grad", "gradian", "gradians"],
    },
    UnitDef {
        symbol: "arcmin",
        name: "arcminute",
        dimension: Dimension::Angle,
        multiplier: (PI / 180.0) / 60.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["arcmin", "arcminute", "arcminutes"],
    },
    UnitDef {
        symbol: "arcsec",
        name: "arcsecond",
        dimension: Dimension::Angle,
        multiplier: (PI / 180.0) / 3600.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["arcsec", "arcsecond", "arcseconds"],
    },
];

const AREA_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "m²",
        name: "square meter",
        dimension: Dimension::Area,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "m²",
            "m2",
            "m^2",
            "sq m",
            "sqm",
            "square meter",
            "square meters",
        ],
    },
    UnitDef {
        symbol: "km²",
        name: "square kilometer",
        dimension: Dimension::Area,
        multiplier: 1e6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "km²",
            "km2",
            "km^2",
            "sq km",
            "sqkm",
            "square kilometer",
            "square kilometers",
        ],
    },
    UnitDef {
        symbol: "cm²",
        name: "square centimeter",
        dimension: Dimension::Area,
        multiplier: 1e-4,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["cm²", "cm2", "cm^2", "sq cm", "sqcm", "square centimeter"],
    },
    UnitDef {
        symbol: "mm²",
        name: "square millimeter",
        dimension: Dimension::Area,
        multiplier: 1e-6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mm²", "mm2", "mm^2", "sq mm", "sqmm", "square millimeter"],
    },
    UnitDef {
        symbol: "ha",
        name: "hectare",
        dimension: Dimension::Area,
        multiplier: 10_000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ha", "hectare", "hectares"],
    },
    UnitDef {
        symbol: "acre",
        name: "acre",
        dimension: Dimension::Area,
        multiplier: 4_046.856_422_4,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["acre", "acres"],
    },
    UnitDef {
        symbol: "sq ft",
        name: "square foot",
        dimension: Dimension::Area,
        multiplier: 0.092_903_04,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "sq ft",
            "sqft",
            "ft²",
            "ft2",
            "ft^2",
            "square foot",
            "square feet",
        ],
    },
    UnitDef {
        symbol: "sq in",
        name: "square inch",
        dimension: Dimension::Area,
        multiplier: 0.000_645_16,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "sq in",
            "sqin",
            "in²",
            "in2",
            "in^2",
            "square inch",
            "square inches",
        ],
    },
    UnitDef {
        symbol: "sq yd",
        name: "square yard",
        dimension: Dimension::Area,
        multiplier: 0.836_127_36,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "sq yd",
            "sqyd",
            "yd²",
            "yd2",
            "yd^2",
            "square yard",
            "square yards",
        ],
    },
    UnitDef {
        symbol: "sq mi",
        name: "square mile",
        dimension: Dimension::Area,
        multiplier: 2_589_988.110_336,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "sq mi",
            "sqmi",
            "mi²",
            "mi2",
            "mi^2",
            "square mile",
            "square miles",
        ],
    },
];

const TIME_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "s",
        name: "second",
        dimension: Dimension::Time,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["s", "sec", "second", "seconds"],
    },
    UnitDef {
        symbol: "ms",
        name: "millisecond",
        dimension: Dimension::Time,
        multiplier: 0.001,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ms", "millisecond", "milliseconds"],
    },
    UnitDef {
        symbol: "min",
        name: "minute",
        dimension: Dimension::Time,
        multiplier: 60.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["min", "minute", "minutes"],
    },
    UnitDef {
        symbol: "h",
        name: "hour",
        dimension: Dimension::Time,
        multiplier: 3600.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["h", "hr", "hour", "hours"],
    },
    UnitDef {
        symbol: "d",
        name: "day",
        dimension: Dimension::Time,
        multiplier: 86_400.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["d", "day", "days"],
    },
    UnitDef {
        symbol: "wk",
        name: "week",
        dimension: Dimension::Time,
        multiplier: 604_800.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["wk", "week", "weeks"],
    },
    UnitDef {
        symbol: "mo",
        name: "month",
        dimension: Dimension::Time,
        multiplier: 2_629_800.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mo", "month", "months"],
    },
    UnitDef {
        symbol: "yr",
        name: "year",
        dimension: Dimension::Time,
        multiplier: 31_557_600.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["yr", "year", "years"],
    },
];

const VOLUME_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "L",
        name: "liter",
        dimension: Dimension::Volume,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["l", "liter", "liters", "litre", "litres"],
    },
    UnitDef {
        symbol: "mL",
        name: "milliliter",
        dimension: Dimension::Volume,
        multiplier: 0.001,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "ml",
            "milliliter",
            "milliliters",
            "millilitre",
            "millilitres",
        ],
    },
    UnitDef {
        symbol: "cl",
        name: "centiliter",
        dimension: Dimension::Volume,
        multiplier: 0.01,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["cl", "centiliter", "centiliters"],
    },
    UnitDef {
        symbol: "dl",
        name: "deciliter",
        dimension: Dimension::Volume,
        multiplier: 0.1,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["dl", "deciliter", "deciliters"],
    },
    UnitDef {
        symbol: "m³",
        name: "cubic meter",
        dimension: Dimension::Volume,
        multiplier: 1000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["m³", "m3", "m^3", "cubic meter", "cubic meters"],
    },
    UnitDef {
        symbol: "cm³",
        name: "cubic centimeter",
        dimension: Dimension::Volume,
        multiplier: 0.001,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &[
            "cm³",
            "cm3",
            "cm^3",
            "cc",
            "cubic centimeter",
            "cubic centimeters",
        ],
    },
    UnitDef {
        symbol: "gal",
        name: "gallon",
        dimension: Dimension::Volume,
        multiplier: 3.785_411_784,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["gal", "gallon", "gallons"],
    },
    UnitDef {
        symbol: "qt",
        name: "quart",
        dimension: Dimension::Volume,
        multiplier: 0.946_352_946,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["qt", "quart", "quarts"],
    },
    UnitDef {
        symbol: "pt",
        name: "pint",
        dimension: Dimension::Volume,
        multiplier: 0.473_176_473,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["pt", "pint", "pints"],
    },
    UnitDef {
        symbol: "cup",
        name: "cup",
        dimension: Dimension::Volume,
        multiplier: 0.236_588_236_5,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["cup", "cups"],
    },
    UnitDef {
        symbol: "fl oz",
        name: "fluid ounce",
        dimension: Dimension::Volume,
        multiplier: 0.029_573_529_562_5,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["fl oz", "floz", "fluid ounce", "fluid ounces"],
    },
    UnitDef {
        symbol: "tbsp",
        name: "tablespoon",
        dimension: Dimension::Volume,
        multiplier: 0.014_786_764_781_25,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["tbsp", "tablespoon", "tablespoons"],
    },
    UnitDef {
        symbol: "tsp",
        name: "teaspoon",
        dimension: Dimension::Volume,
        multiplier: 0.004_928_921_593_75,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["tsp", "teaspoon", "teaspoons"],
    },
];

const ENERGY_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "J",
        name: "joule",
        dimension: Dimension::Energy,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["j", "joule", "joules"],
    },
    UnitDef {
        symbol: "kJ",
        name: "kilojoule",
        dimension: Dimension::Energy,
        multiplier: 1000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kj", "kilojoule", "kilojoules"],
    },
    UnitDef {
        symbol: "MJ",
        name: "megajoule",
        dimension: Dimension::Energy,
        multiplier: 1e6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mj", "megajoule", "megajoules"],
    },
    UnitDef {
        symbol: "cal",
        name: "calorie",
        dimension: Dimension::Energy,
        multiplier: 4.184,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["cal", "calorie", "calories"],
    },
    UnitDef {
        symbol: "kcal",
        name: "kilocalorie",
        dimension: Dimension::Energy,
        multiplier: 4184.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kcal", "kilocalorie", "kilocalories"],
    },
    UnitDef {
        symbol: "Wh",
        name: "watt-hour",
        dimension: Dimension::Energy,
        multiplier: 3600.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["wh", "watt-hour", "watt hour", "watthour"],
    },
    UnitDef {
        symbol: "kWh",
        name: "kilowatt-hour",
        dimension: Dimension::Energy,
        multiplier: 3_600_000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kwh", "kilowatt-hour", "kilowatt hour", "kilowatthour"],
    },
    UnitDef {
        symbol: "BTU",
        name: "British thermal unit",
        dimension: Dimension::Energy,
        multiplier: 1_055.055_85,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["btu"],
    },
    UnitDef {
        symbol: "ft-lb",
        name: "foot-pound",
        dimension: Dimension::Energy,
        multiplier: 1.355_817_948_331_4,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ft-lb", "ft lb", "foot-pound"],
    },
];

const FORCE_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "N",
        name: "newton",
        dimension: Dimension::Force,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["n", "newton", "newtons"],
    },
    UnitDef {
        symbol: "kN",
        name: "kilonewton",
        dimension: Dimension::Force,
        multiplier: 1000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kn", "kilonewton", "kilonewtons"],
    },
    UnitDef {
        symbol: "dyn",
        name: "dyne",
        dimension: Dimension::Force,
        multiplier: 1e-5,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["dyn", "dyne", "dynes"],
    },
    UnitDef {
        symbol: "lbf",
        name: "pound-force",
        dimension: Dimension::Force,
        multiplier: 4.448_221_615_260_5,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["lbf", "pound-force", "pound force"],
    },
];

const POWER_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "W",
        name: "watt",
        dimension: Dimension::Power,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["w", "watt", "watts"],
    },
    UnitDef {
        symbol: "kW",
        name: "kilowatt",
        dimension: Dimension::Power,
        multiplier: 1000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kw", "kilowatt", "kilowatts"],
    },
    UnitDef {
        symbol: "MW",
        name: "megawatt",
        dimension: Dimension::Power,
        multiplier: 1e6,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mw", "megawatt", "megawatts"],
    },
    UnitDef {
        symbol: "hp",
        name: "horsepower",
        dimension: Dimension::Power,
        multiplier: 745.699_871_582_27,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["hp", "horsepower"],
    },
    UnitDef {
        symbol: "ps",
        name: "metric horsepower",
        dimension: Dimension::Power,
        multiplier: 735.498_75,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["ps"],
    },
];

const PRESSURE_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "Pa",
        name: "pascal",
        dimension: Dimension::Pressure,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["pa", "pascal", "pascals"],
    },
    UnitDef {
        symbol: "kPa",
        name: "kilopascal",
        dimension: Dimension::Pressure,
        multiplier: 1000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["kpa", "kilopascal", "kilopascals"],
    },
    UnitDef {
        symbol: "bar",
        name: "bar",
        dimension: Dimension::Pressure,
        multiplier: 100_000.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["bar", "bars"],
    },
    UnitDef {
        symbol: "mbar",
        name: "millibar",
        dimension: Dimension::Pressure,
        multiplier: 100.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mbar", "millibar", "millibars"],
    },
    UnitDef {
        symbol: "psi",
        name: "psi",
        dimension: Dimension::Pressure,
        multiplier: 6_894.757_293_168,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["psi"],
    },
    UnitDef {
        symbol: "atm",
        name: "atmosphere",
        dimension: Dimension::Pressure,
        multiplier: 101_325.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["atm", "atmosphere", "atmospheres"],
    },
    UnitDef {
        symbol: "torr",
        name: "torr",
        dimension: Dimension::Pressure,
        multiplier: 133.322_368_421_05,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["torr", "mmhg"],
    },
];

const FUEL_ECONOMY_UNITS: &[UnitDef] = &[
    UnitDef {
        symbol: "km/L",
        name: "kilometer per liter",
        dimension: Dimension::FuelEconomy,
        multiplier: 1.0,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["km/l", "km/liter", "km per liter"],
    },
    UnitDef {
        symbol: "mpg",
        name: "miles per gallon",
        dimension: Dimension::FuelEconomy,
        multiplier: 0.425_143_707,
        offset: 0.0,
        is_reciprocal: false,
        aliases: &["mpg", "mile per gallon", "miles per gallon"],
    },
    UnitDef {
        symbol: "L/100km",
        name: "liters per 100 kilometers",
        dimension: Dimension::FuelEconomy,
        multiplier: 100.0,
        offset: 0.0,
        is_reciprocal: true,
        aliases: &["l/100km", "liter per 100km", "liters per 100km"],
    },
];

const ALL_UNIT_SLICES: &[&[UnitDef]] = &[
    LENGTH_UNITS,
    MASS_UNITS,
    TEMPERATURE_UNITS,
    DATA_STORAGE_UNITS,
    SPEED_UNITS,
    ANGLE_UNITS,
    AREA_UNITS,
    TIME_UNITS,
    VOLUME_UNITS,
    ENERGY_UNITS,
    FORCE_UNITS,
    POWER_UNITS,
    PRESSURE_UNITS,
    FUEL_ECONOMY_UNITS,
];

/// Finds a unit definition by exact match or case-insensitive alias.
#[must_use]
pub fn find_unit(token: &str) -> Option<&'static UnitDef> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. First attempt exact case match
    for &slice in ALL_UNIT_SLICES {
        for unit in slice {
            if unit.symbol == trimmed {
                return Some(unit);
            }
        }
    }

    // 2. Case-insensitive alias match
    let lower = trimmed.to_lowercase();
    for &slice in ALL_UNIT_SLICES {
        for unit in slice {
            if unit.symbol.eq_ignore_ascii_case(&lower) || unit.name.eq_ignore_ascii_case(&lower) {
                return Some(unit);
            }
            for &alias in unit.aliases {
                if alias.eq_ignore_ascii_case(&lower) {
                    return Some(unit);
                }
            }
        }
    }

    None
}

/// Converts a physical scalar value from a source unit to a target unit.
///
/// # Errors
///
/// Returns [`CalcError::IncompatibleUnits`] if the two units belong to different dimensions,
/// or [`CalcError::Overflow`] if numerical conversion overflows or produces NaN/infinity.
pub fn convert(amount: f64, from: &UnitDef, to: &UnitDef) -> Result<f64, CalcError> {
    if from.dimension != to.dimension {
        return Err(CalcError::IncompatibleUnits(
            from.symbol.to_string(),
            to.symbol.to_string(),
        ));
    }

    let base_val = if from.is_reciprocal {
        if amount <= 0.0 {
            return Err(CalcError::Overflow);
        }
        from.multiplier / amount
    } else {
        amount.mul_add(from.multiplier, from.offset)
    };

    let mut target_val = if to.is_reciprocal {
        if base_val <= 0.0 {
            return Err(CalcError::Overflow);
        }
        to.multiplier / base_val
    } else {
        (base_val - to.offset) / to.multiplier
    };

    if (target_val.round() - target_val).abs() < 1e-11 {
        target_val = target_val.round();
    }

    if target_val.is_nan() || target_val.is_infinite() {
        return Err(CalcError::Overflow);
    }

    Ok(target_val)
}

/// Result of evaluating a unit conversion query.
#[derive(Debug, PartialEq)]
pub struct UnitConversionResult {
    /// Source numeric amount.
    pub source_val: f64,
    /// Canonical source unit symbol.
    pub source_unit: String,
    /// Converted target numeric amount.
    pub target_val: f64,
    /// Canonical target unit symbol.
    pub target_unit: String,
    /// Formatted output string (e.g. `"= 160.934 km"` or `"160.934 km"`).
    pub formatted: String,
}

/// Attempts to parse and evaluate a natural unit conversion phrase.
///
/// Examples:
/// - `100 miles in km`
/// - `32 F to C`
/// - `16GB in MB`
/// - `500 m to ft`
#[must_use]
pub fn try_convert_phrase(query: &str) -> Option<UnitConversionResult> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try separating keywords: " to " and " in "
    let separators = [" to ", " in ", " TO ", " IN "];
    for sep in separators {
        if let Some(idx) = trimmed.find(sep) {
            let left = &trimmed[..idx];
            let right = &trimmed[idx + sep.len()..];

            if let Some(res) = evaluate_split(left, right) {
                return Some(res);
            }
        }
    }

    None
}

fn evaluate_split(left: &str, right: &str) -> Option<UnitConversionResult> {
    let dest_unit = find_unit(right.trim())?;

    // Parse amount and source unit from left
    let left_trimmed = left.trim();
    let (amount, src_unit_str) = parse_amount_and_unit(left_trimmed)?;
    let src_unit = find_unit(src_unit_str)?;

    let target_val = convert(amount, src_unit, dest_unit).ok()?;

    let formatted_target = format_unit_value(target_val);
    let formatted = format!("{formatted_target} {}", dest_unit.symbol);

    Some(UnitConversionResult {
        source_val: amount,
        source_unit: src_unit.symbol.to_string(),
        target_val,
        target_unit: dest_unit.symbol.to_string(),
        formatted,
    })
}

fn parse_amount_and_unit(text: &str) -> Option<(f64, &str)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut end_num = 0;
    let chars: Vec<(usize, char)> = trimmed.char_indices().collect();

    let mut seen_digit = false;
    let mut seen_dot = false;

    for (i, &(byte_idx, ch)) in chars.iter().enumerate() {
        if i == 0 && (ch == '+' || ch == '-') {
            end_num = byte_idx + ch.len_utf8();
            continue;
        }

        if ch.is_ascii_digit() {
            seen_digit = true;
            end_num = byte_idx + ch.len_utf8();
        } else if ch == '.' && !seen_dot {
            seen_dot = true;
            end_num = byte_idx + ch.len_utf8();
        } else if ch == ',' {
            // Thousands separator
            end_num = byte_idx + ch.len_utf8();
        } else {
            break;
        }
    }

    if !seen_digit || end_num == 0 {
        return None;
    }

    let num_raw = &trimmed[..end_num];
    let unit_raw = trimmed[end_num..].trim();

    let num_clean = num_raw.replace(',', "");
    let amount = num_clean.parse::<f64>().ok()?;

    Some((amount, unit_raw))
}

fn format_unit_value(val: f64) -> String {
    if val.fract().abs() < 1e-9 {
        format!("{val:.0}")
    } else {
        // Trim trailing zeros after decimal up to 6 decimal places
        let formatted = format!("{val:.6}");
        let trimmed = formatted.trim_end_matches('0');
        trimmed.trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;

    #[test]
    fn test_length_conversion() {
        let m = find_unit("m").unwrap();
        let km = find_unit("km").unwrap();
        let res = convert(1000.0, m, km).unwrap();
        assert_eq!(res, 1.0);

        let miles = find_unit("miles").unwrap();
        let km_res = convert(100.0, miles, km).unwrap();
        assert!((km_res - 160.9344).abs() < 1e-4);
    }

    #[test]
    fn test_temperature_conversion() {
        let f = find_unit("F").unwrap();
        let c = find_unit("C").unwrap();
        let k = find_unit("K").unwrap();

        assert_eq!(convert(32.0, f, c).unwrap(), 0.0);
        assert_eq!(convert(212.0, f, c).unwrap(), 100.0);
        assert_eq!(convert(0.0, c, f).unwrap(), 32.0);
        assert_eq!(convert(100.0, c, f).unwrap(), 212.0);
        assert_eq!(convert(0.0, c, k).unwrap(), 273.15);
        assert_eq!(convert(273.15, k, c).unwrap(), 0.0);
    }

    #[test]
    fn test_data_storage_conversion() {
        let gb = find_unit("GB").unwrap();
        let mb = find_unit("MB").unwrap();
        assert_eq!(convert(16.0, gb, mb).unwrap(), 16000.0);

        let gib = find_unit("GiB").unwrap();
        let mib = find_unit("MiB").unwrap();
        assert_eq!(convert(1.0, gib, mib).unwrap(), 1024.0);
    }

    #[test]
    fn test_natural_phrases() {
        let res1 = try_convert_phrase("100 miles in km").unwrap();
        assert_eq!(res1.source_val, 100.0);
        assert_eq!(res1.target_unit, "km");
        assert!((res1.target_val - 160.9344).abs() < 1e-4);

        let res2 = try_convert_phrase("32 F to C").unwrap();
        assert_eq!(res2.target_val, 0.0);
        assert_eq!(res2.target_unit, "C");

        let res3 = try_convert_phrase("16GB in MB").unwrap();
        assert_eq!(res3.target_val, 16000.0);
        assert_eq!(res3.target_unit, "MB");
    }

    #[test]
    fn test_incompatible_units() {
        let m = find_unit("m").unwrap();
        let c = find_unit("C").unwrap();
        assert_eq!(
            convert(10.0, m, c),
            Err(CalcError::IncompatibleUnits(
                "m".to_string(),
                "C".to_string()
            ))
        );
    }
}
