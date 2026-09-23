import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const REFERENCE_HEADING = '## Appendix: Language Reference';
const AGENT_START = '<!-- ECKY_AGENT_REFERENCE_START -->';
const AGENT_END = '<!-- ECKY_AGENT_REFERENCE_END -->';

type CampaignLevel = {
  sourceTitle: string;
  title: string;
  mission: string;
  clear: string;
};

const CAMPAIGN_LEVELS: CampaignLevel[] = [
  {
    sourceTitle: 'First Solid: Corner Bracket',
    title: 'Level 01: Corner Bracket',
    mission: 'Build one connected corner bracket from a horizontal foot and a vertical flange.',
    clear: 'Preview shows one connected L-bracket where the foot and flange overlap, and the source compiles.',
  },
  {
    sourceTitle: 'Cut and Join: Mounting Plate',
    title: 'Level 02: Mounting Plate',
    mission: 'Turn a blank into a useful plate with repeated through-holes.',
    clear: 'Every cutter crosses the plate and the exported STL remains one component.',
  },
  {
    sourceTitle: 'Physical Fit: Dovetail Rail and Channel',
    title: 'Level 03: Dovetail Fit',
    mission: 'Make one named fit_clearance drive both mating sides of a dovetail rail and channel.',
    clear: 'Changing fit_clearance widens the channel while the rail stays nominal; no second anonymous offset needs editing.',
  },
  {
    sourceTitle: 'Real Model Patterns: Procedural Cuts and Arrayed Frames',
    title: 'Level 04: Procedural Workshop',
    mission: 'Build generated cutters and path-driven attachments from data.',
    clear: 'One parameter change regenerates the pattern; final geometry still exports as a valid solid.',
  },
  {
    sourceTitle: 'Worked Project: Perforated Toothbrush Holder',
    title: 'Level 05: Perforated Toothbrush Holder',
    mission: 'Build a shelled product, prove one custom cutter, then repeat it across curved walls.',
    clear: 'All four checkpoints compile and the final boolean is one body minus one generated cutter group.',
  },
  {
    sourceTitle: 'Final Model: Integrated Film Adapter Open Helicoid v9',
    title: 'Level 06: Film Adapter',
    mission: 'Read and modify a production-scale multipart mechanism with named fit relations.',
    clear: 'A fit parameter changes both mating sides while preview-only placement leaves export geometry unchanged.',
  },
];

export type EckyIrContentProjection = {
  campaign: string;
  reference: string;
  agentReference: string;
};

export function projectEckyIrContent(corpus: string): EckyIrContentProjection {
  const normalized = corpus.replace(/\r\n/g, '\n');
  const referenceStart = normalized.indexOf(REFERENCE_HEADING);
  if (referenceStart === -1) {
    throw new Error(`Ecky corpus missing ${REFERENCE_HEADING}`);
  }
  const agentStart = normalized.indexOf(AGENT_START);
  const agentEnd = normalized.indexOf(AGENT_END);
  if (agentStart === -1 || agentEnd === -1 || agentEnd <= agentStart) {
    throw new Error('Ecky corpus must contain one bounded agent reference.');
  }

  const lessonCorpus = normalized.slice(0, referenceStart).trim();
  const referenceBody = projectHumanReference(normalized
    .slice(referenceStart + REFERENCE_HEADING.length, agentStart)
    .trim()
    .replace(/^### Generated Operation Index$/m, '## Operation Index'));
  const agentReference = normalized
    .slice(agentStart + AGENT_START.length, agentEnd)
    .trim();

  const campaignSections = CAMPAIGN_LEVELS.map((level) => {
    const body = extractSection(lessonCorpus, level.sourceTitle);
    return [
      `## ${level.title}`,
      '',
      `**Mission:** ${level.mission}`,
      '',
      `**Clear condition:** ${level.clear}`,
      '',
      body,
    ].join('\n');
  });

  return {
    campaign: [
      '# Ecky Campaign',
      '',
      'Learn Ecky as six modeling levels. Each level ends in geometry you can preview,',
      'compile, and export as STL. Finish the clear condition before moving forward;',
      'the dry operation reference lives under `/docs/ecky-ir`.',
      '',
      ...campaignSections,
      '',
    ].join('\n'),
    reference: [
      '# Ecky Language Reference',
      '',
      'Exact forms, signatures, selectors, and verification grammar.',
      '',
      referenceBody,
      '',
    ].join('\n'),
    agentReference: `${agentReference}\n`,
  };
}

function projectHumanReference(markdown: string): string {
  const documented = new Set([...markdown.matchAll(/^#{3,4} `([^`]+)`/gm)].map((match) => match[1]));
  return markdown.replace(/<!-- ECKY_GENERATED_SURFACE_REFERENCE_START -->([\s\S]*?)<!-- ECKY_GENERATED_SURFACE_REFERENCE_END -->/,
    (_, table: string) => {
      const details = table.split('\n').filter((line) => line.startsWith('| `')).flatMap((line) => {
        const columns = line.slice(1, -1).split(/(?<!\\)\|/).map((cell) => cell.trim());
        const name = columns[0].replaceAll('`', '');
        if (documented.has(name)) return [];
        const signature = columns[2].replace(/^`|`$/g, '').replaceAll('\\|', '|');
        const description = columns[4].replaceAll('\\`', '`');
        const example = columns[5].replace(/^`|`$/g, '').replaceAll('\\|', '|');
        return [`### \`${name}\`\n\n\`${signature}\`\n\n${description}\n\nExample fragment:\n\n\`\`\`scheme\n${example}\n\`\`\``];
      }).join('\n\n');
      return details;
    });
}

export function syncEckyIrContent(root: string, check = false): void {
  const corpusPath = path.join(root, 'docs/books/ecky-ir/ecky-ir-corpus.md');
  const projection = projectEckyIrContent(fs.readFileSync(corpusPath, 'utf8'));
  const outputs = [
    [path.join(root, 'public/tutorials/ecky-campaign.md'), projection.campaign],
    [path.join(root, 'public/docs/ecky-ir.md'), projection.reference],
    [path.join(root, 'public/docs/ecky-agent-reference.md'), projection.agentReference],
  ] as const;
  const drift: string[] = [];

  for (const [outputPath, content] of outputs) {
    const current = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, 'utf8') : null;
    if (current === content) continue;
    if (check) {
      drift.push(path.relative(root, outputPath));
      continue;
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, content);
  }

  if (drift.length) {
    throw new Error(`Published Ecky content drifted: ${drift.join(', ')}`);
  }
}

function extractSection(markdown: string, title: string): string {
  const heading = `## ${title}`;
  const start = markdown.indexOf(heading);
  if (start === -1) throw new Error(`Ecky corpus missing campaign section: ${heading}`);
  const bodyStart = start + heading.length;
  const next = markdown.indexOf('\n## ', bodyStart);
  return markdown.slice(bodyStart, next === -1 ? markdown.length : next).trim();
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : '';
if (invokedPath === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes('--check');
  syncEckyIrContent(process.cwd(), check);
  console.log(check ? 'Published Ecky content is current.' : 'Published Ecky content synchronized.');
}

const missionFiles = [
  ['mission-01-bracket-enclosure', 'level-01-corner-bracket.md'],
  ['mission-02-bottle-cage-dovetail', 'level-02-bottle-cage-dovetail.md'],
  ['mission-03-wing-propeller-study', 'level-03-printable-wing-propeller.md'],
  ['mission-04-gillette-travel-kit', 'level-04-gillette-travel-kit.md'],
  ['mission-05-iphone-case-fixture', 'level-05-iphone-case-fixture.md'],
  ['mission-06-film-scanner', 'level-06-film-scanner.md'],
] as const;

export function readPublishedChapters(root: string, sourceBase = '/docs') {
  const exampleFiles = new Map<string, Buffer>();
  const chapters = missionFiles.map(([id, file]) => {
    const contentPath = path.join(root, 'docs', 'books', 'ecky-ir', 'missions', file);
    const markdown = fs.readFileSync(contentPath, 'utf8').replace(/\[([^\]]+)\]\(([^)]+\.ecky)\)/g, (_, label, relativePath) => {
      const sourcePath = path.resolve(path.dirname(contentPath), relativePath);
      if (!sourcePath.startsWith(`${root}${path.sep}`)) throw new Error(`Chapter source outside repository: ${relativePath}`);
      const bytes = fs.readFileSync(sourcePath);
      const hash = createHash('sha256').update(bytes).digest('hex').slice(0, 12);
      const assetPath = `examples/${hash}-${path.basename(sourcePath)}`;
      exampleFiles.set(assetPath, bytes);
      return `[${label}](${sourceBase}/${assetPath})`;
    });
    const title = markdown.match(/^title:\s*(.+)$/m)?.[1] ?? id;
    return { id, sectionSlug: file.replace(/\.md$/, ''), title, markdown, checkpoints: [] };
  });

  return { chapters, exampleFiles };
}
