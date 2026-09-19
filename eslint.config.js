import js from "@eslint/js";
import pluginVue from "eslint-plugin-vue";
import tseslint from "typescript-eslint";

export default [
  {
    ignores: [
      ".tmp/**",
      ".tools/**",
      ".vcpkg_installed/**",
      "dist/**",
      "evidence/artifacts/**",
      "evidence/installers/**",
      "node_modules/**",
      "runtime/diagnostics/**",
      "runtime/staging/**",
      "src-tauri/gen/**",
      "target/**",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...pluginVue.configs["flat/essential"],
  {
    files: ["**/*.vue"],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
  },
  {
    files: ["tools/**/*.mjs"],
    languageOptions: {
      globals: {
        console: "readonly",
        process: "readonly",
        URL: "readonly",
      },
    },
  },
  {
    languageOptions: {
      globals: {
        document: "readonly",
      },
    },
  },
];
