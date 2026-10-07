import os

/// Structured logging via Apple's unified logging.
///
/// No scattered `print()` calls. Filter in Console.app / `log stream` with
/// subsystem `com.scirats.aqua` and the relevant category.
enum Log {
    private static let subsystem = "com.scirats.aqua"

    static let app = Logger(subsystem: subsystem, category: "app")
    static let scene = Logger(subsystem: subsystem, category: "scene")
    static let window = Logger(subsystem: subsystem, category: "window")
    static let service = Logger(subsystem: subsystem, category: "service")
    static let viewport = Logger(subsystem: subsystem, category: "viewport")
    static let input = Logger(subsystem: subsystem, category: "input")
    static let clipboard = Logger(subsystem: subsystem, category: "clipboard")
    static let surface = Logger(subsystem: subsystem, category: "surface")
}
