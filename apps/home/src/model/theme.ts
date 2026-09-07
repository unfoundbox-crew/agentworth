import type { Role } from '../protocol';

/**
 * A theme is data: it renames roles and spaces, sets a badge colour, and
 * supplies a voice file. Components never know a theme's characters.
 */
export interface Character {
  name: string;
  title: string;
  /** Badge hue; the only colour a persona owns. */
  badge: string;
  /** Path of the soul file the gateway loads for this role, relative to the theme. */
  soul?: string;
}

export interface Theme {
  id: string;
  label: string;
  cast: Record<Role, Character>;
  spaces: Record<string, string>;
}

/** No culture at all. What the app shows until a theme is picked. */
export const plain: Theme = {
  id: 'plain',
  label: 'Plain',
  cast: {
    chief_of_staff: { name: 'Chief of staff', title: 'triage and routing', badge: '#60a5fa' },
    senior: { name: 'Senior', title: 'architecture and hard calls', badge: '#c9a227' },
    associate: { name: 'Associate', title: 'research and audits', badge: '#10b981' },
    executor: { name: 'Executor', title: 'builds and PRs', badge: '#c084fc' },
    controller: { name: 'Controller', title: 'tokens and pacing', badge: '#a38241' },
    guest: { name: 'Guest', title: '', badge: '#8b949e' },
  },
  spaces: {
    incidents: 'Incidents',
    lounge: 'Lounge',
    standup: 'Standup',
  },
};

export function characterFor(theme: Theme, role: Role): Character {
  return theme.cast[role] ?? theme.cast.guest;
}
