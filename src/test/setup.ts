import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Vitest globals are disabled: Testing Library cannot register automatic
// cleanup through a global afterEach. Make component/hook ownership explicit.
afterEach(cleanup);
