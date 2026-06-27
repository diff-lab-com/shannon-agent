// Tests for Profiles page — within-profile conflict detection + create-form
// duplicate-name / rule-conflict validation.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { IntlProvider } from 'react-intl'
import Profiles from '@/pages/Profiles'
import type { CustomProfileInfo } from '@/types'

const listSpy = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri-api')>('@/lib/tauri-api')
  return {
    ...actual,
    listPermissionProfiles: (...a: unknown[]) => listSpy(...a),
    saveCustomProfile: vi.fn(async () => {}),
    deleteCustomProfile: vi.fn(async () => {}),
  }
})
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

const M: Record<string, string> = {
  'profiles.title': 'Profiles', 'profiles.subtitle': '', 'profiles.subtitle.code': '',
  'profiles.subtitle.end': '', 'profiles.subtitle.path': '', 'profiles.subtitle.end2': '',
  'profiles.newProfile': 'New profile', 'profiles.cancel': 'Cancel',
  'profiles.builtin': 'Builtin', 'profiles.custom': 'Custom', 'profiles.count': '{count}',
  'profiles.empty.title': 'None', 'profiles.empty.description': 'No custom profiles',
  'profiles.rule.auto': 'Auto', 'profiles.rule.confirm': 'Confirm', 'profiles.rule.deny': 'Deny',
  'profiles.conflict.ruleConflict': 'Conflicting rules — {tools}',
  'profiles.conflict.duplicateName': 'A profile with this name already exists.',
  'profiles.new.title': 'New profile',
  'profiles.form.name': 'Name', 'profiles.form.name.hint': '', 'profiles.form.name.placeholder': '',
  'profiles.form.description': 'Description', 'profiles.form.description.placeholder': '',
  'profiles.form.autoApprove': 'Auto-approve', 'profiles.form.autoApprove.hint': '', 'profiles.form.autoApprove.placeholder': '',
  'profiles.form.confirm': 'Confirm', 'profiles.form.confirm.hint': '', 'profiles.form.confirm.placeholder': '',
  'profiles.form.deny': 'Deny', 'profiles.form.deny.hint': '', 'profiles.form.deny.placeholder': '',
  'profiles.form.cancel': 'Cancel', 'profiles.form.create': 'Create', 'profiles.form.saving': 'Saving',
  'profiles.delete.aria': 'Delete {name}',
  'profiles.confirmDelete.title': 'Delete?', 'profiles.confirmDelete.message': 'Delete {name}?',
  'profiles.confirmDelete.confirm': 'Delete', 'profiles.confirmDelete.cancel': 'Cancel',
  'profiles.error.nameRequired': 'Name required', 'profiles.error.load': 'Load failed',
  'profiles.toast.created': 'Created {name}',
}

function renderP() {
  return render(
    <IntlProvider locale="en" messages={M} defaultLocale="en">
      <Profiles />
    </IntlProvider>,
  )
}

function custom(p: Partial<CustomProfileInfo> & { name: string }): CustomProfileInfo {
  return { description: '', auto_approve: [], confirm: [], deny: [], ...p } as CustomProfileInfo
}

beforeEach(() => {
  listSpy.mockReset()
  listSpy.mockResolvedValue({ builtin: [], custom: [] })
})

describe('Profiles — within-profile conflict detection', () => {
  it('flags a profile whose tool is in both Deny and an allow list', async () => {
    listSpy.mockResolvedValue({ builtin: [], custom: [custom({ name: 'risky', auto_approve: ['Bash'], deny: ['Bash'] })] })
    renderP()
    expect(await screen.findByText(/Conflicting rules/)).toBeInTheDocument()
    // 'Bash' renders in both the Auto chip and the Deny chip, and is named in
    // the warning — at least two occurrences prove it is in both lists.
    expect(screen.getAllByText(/Bash/).length).toBeGreaterThanOrEqual(2)
  })

  it('does not flag a profile with non-overlapping rules', async () => {
    listSpy.mockResolvedValue({ builtin: [], custom: [custom({ name: 'clean', auto_approve: ['Read'], confirm: ['Write'], deny: ['Bash'] })] })
    renderP()
    await waitFor(() => expect(screen.getByText('clean')).toBeInTheDocument())
    expect(screen.queryByText(/Conflicting rules/)).not.toBeInTheDocument()
  })

  it('flags a conflict across confirm + deny (not just auto-approve)', async () => {
    listSpy.mockResolvedValue({ builtin: [], custom: [custom({ name: 'mix', confirm: ['Edit'], deny: ['Edit'] })] })
    renderP()
    expect(await screen.findByText(/Conflicting rules/)).toBeInTheDocument()
  })
})

describe('Profiles — create-form validation', () => {
  // input order in the form: [0]=name [1]=description [2]=auto_approve [3]=confirm [4]=deny
  function openForm() {
    fireEvent.click(screen.getByText('New profile'))
    return screen.getAllByRole('textbox')
  }

  it('blocks save and warns when the name duplicates an existing profile', async () => {
    listSpy.mockResolvedValue({ builtin: [], custom: [custom({ name: 'exists' })] })
    renderP()
    await waitFor(() => expect(screen.getByText('exists')).toBeInTheDocument())
    const inputs = openForm()
    fireEvent.change(inputs[0], { target: { value: 'exists' } })
    // The duplicate-name warning renders both inline under the name field and
    // in the footer — at least one occurrence proves it surfaced.
    expect(screen.getAllByText(/already exists/).length).toBeGreaterThanOrEqual(1)
    expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled()
  })

  it('enables save for a unique name with non-conflicting rules', async () => {
    renderP()
    await waitFor(() => expect(screen.getByText('No custom profiles')).toBeInTheDocument())
    const inputs = openForm()
    fireEvent.change(inputs[0], { target: { value: 'brand-new' } })
    expect(screen.queryByText(/already exists/)).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Create' })).not.toBeDisabled()
  })

  it('blocks save when a denied tool also appears in an allow list', async () => {
    renderP()
    await waitFor(() => expect(screen.getByText('No custom profiles')).toBeInTheDocument())
    const inputs = openForm()
    fireEvent.change(inputs[0], { target: { value: 'new1' } })          // unique name
    fireEvent.change(inputs[4], { target: { value: 'Read' } })          // deny 'Read' — overlaps default auto_approve
    expect(screen.getByText(/Conflicting rules/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled()
  })
})
