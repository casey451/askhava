// TypeScript lint for the browser extras and the Playwright suites (D-453; docs/architecture.md section 11).
// Type-aware strict rules on every .ts file; generated .js beside a static .ts source, Rust build output
// and the Python folders are not linted.
import { defineConfig } from "eslint/config";
import tseslint from "typescript-eslint";

export default defineConfig(
  {
    ignores: ["node_modules/", "target/", "twin/", "stand_ins/", "app/*/static/**/*.js", "test-results/", "playwright-report/"],
  },
  {
    files: ["**/*.ts"],
    extends: [tseslint.configs.strictTypeChecked, tseslint.configs.stylisticTypeChecked],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
  },
);
