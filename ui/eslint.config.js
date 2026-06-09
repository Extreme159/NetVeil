import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      globals: globals.browser,
    },
    rules: {
      // _ prefix'li değişkenler intentional unused (destruct-to-ignore pattern)
      "@typescript-eslint/no-unused-vars": ["error", { "argsIgnorePattern": "^_", "varsIgnorePattern": "^_" }],
      // setState-in-effect: external event'e response veya EMA/timer pattern'leri için false positive üretiyor.
      // Bu projedeki tüm setState-in-effect kullanımları bilinçli ve doğru: live data feed'e tepki,
      // cleanup, timer interval içinde update. Rule devre dışı bırakılıyor.
      "react-hooks/set-state-in-effect": "off",
    },
  },
])
