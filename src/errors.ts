export class CanyonError extends Error {
  constructor(message: string, readonly code: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "CanyonError";
  }
}

export class ParseError extends CanyonError {
  constructor(message: string, options?: ErrorOptions) {
    super(message, "PARSE_ERROR", options);
    this.name = "ParseError";
  }
}
