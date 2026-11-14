export type ChannelSuggestion = {
  id: string;
  name: string;
  avatar_url?: string | null;
  handle?: string | null;
  media_count: number;
};

export function selectChannelSuggestions(
  channels: ChannelSuggestion[],
  rawQuery: string,
  limit = 3,
  hiddenTerms: string[] = []
): ChannelSuggestion[] {
  const query = rawQuery.trim().toLowerCase();
  if (!query || limit <= 0) return [];
  const normalizedHiddenTerms = hiddenTerms
    .map((term) => term.trim().replace(/^#/, '').toLowerCase())
    .filter(Boolean);

  const rank = (channel: ChannelSuggestion): number => {
    const name = channel.name.toLowerCase();
    const handle = channel.handle?.toLowerCase() ?? '';
    if (name === query || handle === query) return 0;
    if (name.startsWith(query) || handle.startsWith(query)) return 1;
    return 2;
  };

  return channels
    .filter((channel) => {
      const name = channel.name.toLowerCase();
      const handle = channel.handle?.toLowerCase() ?? '';
      const isHidden = normalizedHiddenTerms.some(
        (term) => name.includes(term) || handle.includes(term)
      );
      return !isHidden && (name.includes(query) || handle.includes(query));
    })
    .sort((left, right) => {
      const rankDifference = rank(left) - rank(right);
      if (rankDifference !== 0) return rankDifference;
      const countDifference = right.media_count - left.media_count;
      if (countDifference !== 0) return countDifference;
      return left.name.localeCompare(right.name);
    })
    .slice(0, limit);
}
