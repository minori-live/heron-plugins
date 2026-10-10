import { defineConfig } from "oxlint"

export default defineConfig({
  plugins: ["typescript"],
  categories: { correctness: "error" },
  env: { node: true },
  ignorePatterns: ["target/**", "coverage/**", "dist/**", "node_modules/**", "third_party/**"]
})
