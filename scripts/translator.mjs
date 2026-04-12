import fs from 'fs-extra';
import path from 'path';
import { fileURLToPath } from 'url';
import xlsx from 'xlsx';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const args = process.argv.slice(2);
const excelPath = args[0]
  ? path.resolve(process.cwd(), args[0])
  : path.resolve(__dirname, 'translations.xlsx');
const outputDirs =
  args.length > 1
    ? args.slice(1).map(outputDir => path.resolve(process.cwd(), outputDir))
    : [
        path.resolve(__dirname, '../apps/manager-tauri/public/locales'),
        path.resolve(__dirname, '../apps/canvas-mirror-viewer/locales'),
      ];

if (!fs.existsSync(excelPath)) {
  console.error(`Excel file not found: ${excelPath}`);
  process.exit(1);
}

const workbook = xlsx.readFile(excelPath);

for (const namespace of workbook.SheetNames) {
  const sheet = workbook.Sheets[namespace];
  const jsonData = xlsx.utils.sheet_to_json(sheet);

  if (jsonData.length === 0) {
    continue;
  }

  const languages = Object.keys(jsonData[0]).filter((column) => column !== 'key');

  for (const outputDir of outputDirs) {
    for (const language of languages) {
      const languageDirectory = path.join(outputDir, language);
      fs.ensureDirSync(languageDirectory);

      const translations = {};

      for (const row of jsonData) {
        if (!row.key) {
          continue;
        }

        translations[row.key] = row[language] || '';
      }

      const jsonPath = path.join(languageDirectory, `${namespace}.json`);
      fs.writeJsonSync(jsonPath, translations, { spaces: 2, encoding: 'utf-8' });

      console.log(`Generated: ${jsonPath}`);
    }
  }
}

console.log('Translation files generated successfully.');
