import { TrustLevel } from './types';

export const TRUST_LEVELS: { value: TrustLevel; labelKey: string }[] = [
  { value: 'Tofu', labelKey: 'contacts.trustLevels.tofu' },
  { value: 'CodeVerified', labelKey: 'contacts.trustLevels.codeVerified' },
  { value: 'Vouched', labelKey: 'contacts.trustLevels.vouched' },
  { value: 'VerifiedInPerson', labelKey: 'contacts.trustLevels.verifiedInPerson' },
];
