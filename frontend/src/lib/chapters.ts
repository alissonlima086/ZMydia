export interface Chapter {
  id: number;
  startTime: number;
  endTime: number;
  timeFormatted: string;
  title: string;
}

/**
 * Converte string de timestamp "MM:SS" ou "HH:MM:SS" para segundos.
 */
export function parseTimestampToSeconds(timeStr: string): number | null {
  const parts = timeStr.trim().split(':');
  if (parts.length === 2) {
    const mins = parseInt(parts[0], 10);
    const secs = parseInt(parts[1], 10);
    if (!isNaN(mins) && !isNaN(secs)) {
      return mins * 60 + secs;
    }
  } else if (parts.length === 3) {
    const hours = parseInt(parts[0], 10);
    const mins = parseInt(parts[1], 10);
    const secs = parseInt(parts[2], 10);
    if (!isNaN(hours) && !isNaN(mins) && !isNaN(secs)) {
      return hours * 3600 + mins * 60 + secs;
    }
  }
  return null;
}

/**
 * Extrai capítulos da descrição de um vídeo do YouTube.
 * Exige pelo menos 2 capítulos com o primeiro iniciando em até 5 segundos.
 */
export function parseChapters(description: string | null | undefined, durationSec = 0): Chapter[] {
  if (!description) return [];

  // Regex para capturar timestamps no início da linha ou após quebra de linha:
  // Exemplos: "0:00 Intro", "[01:20] How it works", "0:22 - Mechanics"
  const regex = /(?:^|\n)\s*(?:(?:\[|\()?\s*(\d{1,2}:\d{2}(?::\d{2})?)\s*(?:\]|\))?)\s*[-–—:•|]?\s*(.+?)(?=\n|$)/g;

  const rawMatches: { time: number; timeFormatted: string; title: string }[] = [];
  let match: RegExpExecArray | null;

  while ((match = regex.exec(description)) !== null) {
    const timeFormatted = match[1].trim();
    let rawTitle = match[2].trim();

    // Remove eventuais timestamps residuais no título (ex: "0:00 - 0:22 Title")
    rawTitle = rawTitle.replace(/^\d{1,2}:\d{2}(?::\d{2})?\s*[-–—:•|]?\s*/, '').trim();

    const timeSec = parseTimestampToSeconds(timeFormatted);
    if (timeSec !== null && rawTitle) {
      rawMatches.push({
        time: timeSec,
        timeFormatted,
        title: rawTitle
      });
    }
  }

  // Validação: requer pelo menos 2 capítulos e o primeiro deve ser <= 5s
  if (rawMatches.length < 2) return [];

  // Ordena cronologicamente
  rawMatches.sort((a, b) => a.time - b.time);

  if (rawMatches[0].time > 5) return [];

  // Remove timestamps duplicados
  const uniqueMatches: typeof rawMatches = [];
  for (const m of rawMatches) {
    if (!uniqueMatches.some((u) => u.time === m.time)) {
      uniqueMatches.push(m);
    }
  }

  if (uniqueMatches.length < 2) return [];

  const chapters: Chapter[] = [];
  const maxDur = durationSec > 0 ? durationSec : uniqueMatches[uniqueMatches.length - 1].time + 60;

  for (let i = 0; i < uniqueMatches.length; i++) {
    const curr = uniqueMatches[i];
    const next = uniqueMatches[i + 1];
    const endTime = next ? next.time : Math.max(curr.time + 1, maxDur);

    chapters.push({
      id: i + 1,
      startTime: curr.time,
      endTime,
      timeFormatted: curr.timeFormatted,
      title: curr.title
    });
  }

  return chapters;
}

export interface NoteLinePart {
  type: 'text' | 'timestamp';
  content: string;
  timeSec?: number;
}

/**
 * Divide uma linha de texto identificando timestamps para torná-los interativos.
 */
export function parseNoteLine(line: string): NoteLinePart[] {
  const tsRegex = /(?:\[|\()?\b(\d{1,2}:\d{2}(?::\d{2})?)\b(?:\]|\))?/g;
  const parts: NoteLinePart[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = tsRegex.exec(line)) !== null) {
    if (match.index > lastIndex) {
      parts.push({
        type: 'text',
        content: line.slice(lastIndex, match.index)
      });
    }
    const timeSec = parseTimestampToSeconds(match[1]);
    if (timeSec !== null) {
      parts.push({
        type: 'timestamp',
        content: match[0],
        timeSec
      });
    } else {
      parts.push({
        type: 'text',
        content: match[0]
      });
    }
    lastIndex = tsRegex.lastIndex;
  }

  if (lastIndex < line.length) {
    parts.push({
      type: 'text',
      content: line.slice(lastIndex)
    });
  }

  return parts;
}
