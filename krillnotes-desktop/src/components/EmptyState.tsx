// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

import { useTranslation } from 'react-i18next';

function EmptyState() {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-center min-h-screen">
      <div className="text-center">
        <img
          src="/KrillNotesLogo.png"
          alt="KrillNotes"
          className="w-64 h-64 mx-auto mb-6 object-contain"
        />
        <p className="text-muted-foreground">
          {t('empty.getStarted')}
        </p>
      </div>
    </div>
  );
}

export default EmptyState;
