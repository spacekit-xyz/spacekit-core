/**
 * Minimal Bun global type declarations.
 *
 * Only the subset used by SpaceKit entry points is declared here.
 * Install `@types/bun` for full typings when developing Bun-specific code.
 */

declare const Bun: {
  argv: string[];
  serve(options: {
    port: number;
    hostname?: string;
    fetch(req: Request): Response | Promise<Response>;
  }): {
    port: number;
    hostname: string;
    stop(): void;
  };
};
