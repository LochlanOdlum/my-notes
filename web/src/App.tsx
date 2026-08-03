import { useCallback, useEffect, useMemo, useState } from 'react'

const publishedBaseUrl = (
  import.meta.env.VITE_PUBLISHED_CONTENT_URL ??
  'https://d81ul6xa7pt91.cloudfront.net'
).replace(/\/$/, '')
const adminBaseUrl = (
  import.meta.env.VITE_ADMIN_API_URL ??
  'https://hzu0shchx5.execute-api.eu-west-2.amazonaws.com'
).replace(/\/$/, '')

type FolderNode = {
  type: 'folder'
  id: string
  parentId: string | null
  title: string
  position: number
}

type NoteNode = {
  type: 'note'
  id: string
  parentId: string | null
  title: string
  slug: string
  position: number
  status: 'draft' | 'published'
  publishedRevision?: string
}

type TreeManifest = {
  nodes: Array<FolderNode | NoteNode>
}

type DocumentNode = {
  type?: string
  text?: string
  attrs?: { level?: number }
  content?: DocumentNode[]
}

type PublishedNote = {
  document: DocumentNode
}

type PublishedNoteResponse = {
  revision: string
  publicPath: string
}

function textContent(node: DocumentNode): string {
  if (node.type === 'text') return node.text ?? ''
  return node.content?.map(textContent).join('') ?? ''
}

function documentToText(document: DocumentNode): string {
  return document.content?.map(textContent).join('\n\n') ?? ''
}

function textToDocument(text: string): DocumentNode {
  return {
    type: 'doc',
    content: text.split(/\n{2,}/).map((paragraph) => ({
      type: 'paragraph',
      content: paragraph ? [{ type: 'text', text: paragraph }] : [],
    })),
  }
}

function renderBlock(node: DocumentNode, key: number): React.ReactNode {
  const content = node.content?.map((child, index) => renderBlock(child, index))

  switch (node.type) {
    case 'text':
      return node.text
    case 'heading': {
      const level = Math.min(Math.max(node.attrs?.level ?? 2, 1), 3)
      const Heading = `h${level}` as 'h1' | 'h2' | 'h3'
      return <Heading key={key}>{content}</Heading>
    }
    case 'paragraph':
      return <p key={key}>{content}</p>
    case 'bulletList':
    case 'bullet_list':
      return <ul key={key}>{content}</ul>
    case 'orderedList':
    case 'ordered_list':
      return <ol key={key}>{content}</ol>
    case 'listItem':
    case 'list_item':
      return <li key={key}>{content}</li>
    case 'blockquote':
      return <blockquote key={key}>{content}</blockquote>
    case 'codeBlock':
    case 'code_block':
      return <pre key={key}>{textContent(node)}</pre>
    case 'horizontalRule':
    case 'horizontal_rule':
      return <hr key={key} />
    default:
      return <div key={key}>{content}</div>
  }
}

function noteSlugFromHash() {
  return decodeURIComponent(window.location.hash.slice(1))
}

function AdminPanel({ onClose }: { onClose: () => void }) {
  const [tree, setTree] = useState<TreeManifest | null>(null)
  const [selected, setSelected] = useState<NoteNode | null>(null)
  const [note, setNote] = useState<PublishedNote | null>(null)
  const [message, setMessage] = useState('Loading private notes…')
  const [publishing, setPublishing] = useState(false)
  const [saving, setSaving] = useState(false)
  const [creating, setCreating] = useState(false)
  const [title, setTitle] = useState('')
  const [slug, setSlug] = useState('')
  const [draftEtag, setDraftEtag] = useState<string | null>(null)
  const [editorText, setEditorText] = useState('')

  const loadTree = useCallback(() => {
    setMessage('Loading private notes…')
    return fetch(`${adminBaseUrl}/admin/tree`)
      .then(async (response) => {
        if (!response.ok) throw new Error(`Admin API unavailable (${response.status}).`)
        return response.json() as Promise<TreeManifest>
      })
      .then((nextTree) => {
        setTree(nextTree)
        setMessage('')
        return nextTree
      })
      .catch((error: unknown) => {
        setMessage(error instanceof Error ? error.message : 'Unable to load private notes.')
        return null
      })
  }, [])

  useEffect(() => {
    void loadTree()
  }, [loadTree])

  const notes = useMemo(
    () => (tree?.nodes.filter((node): node is NoteNode => node.type === 'note') ?? []).sort((a, b) => a.position - b.position),
    [tree],
  )

  const selectNote = (candidate: NoteNode) => {
    setSelected(candidate)
    setNote(null)
    setMessage('Loading draft…')
    fetch(`${adminBaseUrl}/admin/notes/${candidate.id}`)
      .then(async (response) => {
        if (!response.ok) throw new Error(`Draft unavailable (${response.status}).`)
        return { draft: await response.json() as PublishedNote, etag: response.headers.get('etag') }
      })
      .then(({ draft, etag }) => {
        setNote(draft)
        setDraftEtag(etag)
        setEditorText(documentToText(draft.document))
        setMessage('')
      })
      .catch((error: unknown) => setMessage(error instanceof Error ? error.message : 'Unable to load draft.'))
  }

  const saveDraft = async () => {
    if (!selected || !draftEtag) return
    setSaving(true)
    setMessage('Saving…')
    try {
      const response = await fetch(`${adminBaseUrl}/admin/notes/${selected.id}/draft`, {
        method: 'PUT',
        // No custom headers: this remains a CORS-simple request while auth is
        // disabled for local development. The API also accepts this ETag in
        // the body and retains the standard If-Match header for other clients.
        body: JSON.stringify({ document: textToDocument(editorText), etag: draftEtag }),
      })
      if (response.status === 409) throw new Error('This draft changed elsewhere. Reload it before saving.')
      if (!response.ok) throw new Error(`Saving failed (${response.status}).`)
      const updated = await response.json() as PublishedNote
      setNote(updated)
      setDraftEtag(response.headers.get('etag'))
      setMessage('Saved.')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Saving failed.')
    } finally {
      setSaving(false)
    }
  }

  const publish = async () => {
    if (!selected) return
    setPublishing(true)
    setMessage('Publishing…')
    try {
      const response = await fetch(`${adminBaseUrl}/admin/notes/${selected.id}/publish`, { method: 'POST' })
      if (!response.ok) throw new Error(`Publishing failed (${response.status}).`)
      const result = await response.json() as PublishedNoteResponse
      const published = { ...selected, status: 'published' as const, publishedRevision: result.revision }
      setSelected(published)
      setTree((current) => current && {
        ...current,
        nodes: current.nodes.map((node) => node.type === 'note' && node.id === published.id ? published : node),
      })
      setMessage(`Published at ${result.publicPath}`)
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Publishing failed.')
    } finally {
      setPublishing(false)
    }
  }

  const createNote = async (event: React.FormEvent) => {
    event.preventDefault()
    setCreating(true)
    setMessage('Creating note…')
    try {
      const response = await fetch(`${adminBaseUrl}/admin/notes`, {
        method: 'POST',
        // Sending JSON without an explicit application/json header keeps this
        // development request CORS-simple. The Lambda parses the JSON body
        // independently of the content type.
        body: JSON.stringify({ title, slug }),
      })
      if (!response.ok) throw new Error(`Creating the note failed (${response.status}).`)
      const created = await response.json() as { id: string }
      setTitle('')
      setSlug('')
      const nextTree = await loadTree()
      const createdNote = nextTree?.nodes.find((node): node is NoteNode => node.type === 'note' && node.id === created.id)
      if (createdNote) selectNote(createdNote)
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Creating the note failed.')
    } finally {
      setCreating(false)
    }
  }

  return (
    <section className="admin-workspace">
      <div className="admin-heading">
        <div><p className="eyebrow">Admin</p><h1>Private notes</h1></div>
        <button className="secondary-button" onClick={onClose}>View public site</button>
      </div>
      <div className="admin-layout">
        <aside className="admin-list">
          <form className="create-note" onSubmit={createNote}>
            <label>Title<input value={title} onChange={(event) => setTitle(event.target.value)} required /></label>
            <label>Slug<input value={slug} onChange={(event) => setSlug(event.target.value)} pattern="[a-z0-9]+(?:-[a-z0-9]+)*" required /></label>
            <button className="publish-button" disabled={creating} type="submit">{creating ? 'Creating…' : 'New note'}</button>
          </form>
          <nav aria-label="Private notes">
          {notes.map((candidate) => (
            <button className={candidate.id === selected?.id ? 'note-link active' : 'note-link'} key={candidate.id} onClick={() => selectNote(candidate)}>
              <span>{candidate.title}</span><small>{candidate.status}</small>
            </button>
          ))}
          {tree && notes.length === 0 ? <p className="status">No notes yet.</p> : null}
          </nav>
        </aside>
        <div className="admin-document">
          {selected ? <><p className="eyebrow">{selected.status}</p><h2>{selected.title}</h2></> : null}
          {note ? <>
            <textarea className="note-editor" aria-label="Note content" value={editorText} onChange={(event) => setEditorText(event.target.value)} />
            <button className="save-button" disabled={saving} onClick={saveDraft}>{saving ? 'Saving…' : 'Save draft'}</button>
            <section className="prose preview">{note.document.content?.map(renderBlock)}</section>
          </> : null}
          {selected?.status === 'draft' ? <button className="publish-button" disabled={publishing} onClick={publish}>{publishing ? 'Publishing…' : 'Publish page'}</button> : null}
          {message ? <p className={message.includes('failed') || message.includes('unavailable') ? 'status error' : 'status'}>{message}</p> : null}
        </div>
      </div>
    </section>
  )
}

function App() {
  const [tree, setTree] = useState<TreeManifest | null>(null)
  const [treeError, setTreeError] = useState<string | null>(null)
  const [selectedSlug, setSelectedSlug] = useState(noteSlugFromHash)
  const [note, setNote] = useState<PublishedNote | null>(null)
  const [noteError, setNoteError] = useState<string | null>(null)
  const [adminOpen, setAdminOpen] = useState(false)

  useEffect(() => {
    const controller = new AbortController()
    fetch(`${publishedBaseUrl}/tree.json`, { signal: controller.signal })
      .then(async (response) => {
        // A private S3 origin responds with 403 for a missing tree.json. Until
        // the first publish, that means the public site is simply empty.
        if (response.status === 403 || response.status === 404) return { nodes: [] }
        if (!response.ok) throw new Error(`The public tree is unavailable (${response.status}).`)
        return response.json() as Promise<TreeManifest>
      })
      .then(setTree)
      .catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === 'AbortError')) {
          setTreeError(error instanceof Error ? error.message : 'Unable to load published notes.')
        }
      })
    return () => controller.abort()
  }, [])

  useEffect(() => {
    const onHashChange = () => setSelectedSlug(noteSlugFromHash())
    window.addEventListener('hashchange', onHashChange)
    return () => window.removeEventListener('hashchange', onHashChange)
  }, [])

  const notes = useMemo(
    () =>
      (tree?.nodes.filter((node): node is NoteNode => node.type === 'note' && node.status === 'published' && Boolean(node.publishedRevision)) ?? []).sort(
        (left, right) => left.position - right.position,
      ),
    [tree],
  )
  const selectedNote = notes.find((candidate) => candidate.slug === selectedSlug) ?? notes[0]

  useEffect(() => {
    if (!selectedNote) return
    const controller = new AbortController()
    setNote(null)
    setNoteError(null)
    fetch(
      `${publishedBaseUrl}/notes/${selectedNote.id}/${selectedNote.publishedRevision}.json`,
      { signal: controller.signal },
    )
      .then(async (response) => {
        if (!response.ok) throw new Error(`This note is unavailable (${response.status}).`)
        return response.json() as Promise<PublishedNote>
      })
      .then(setNote)
      .catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === 'AbortError')) {
          setNoteError(error instanceof Error ? error.message : 'Unable to load this note.')
        }
      })
    return () => controller.abort()
  }, [selectedNote?.id, selectedNote?.publishedRevision])

  const selectNote = (selected: NoteNode) => {
    window.location.hash = encodeURIComponent(selected.slug)
  }

  return (
    <main className="site-shell">
      <aside className="sidebar">
        <a className="brand" href="#">
          My Notes
        </a>
        <button className="admin-toggle" onClick={() => setAdminOpen(true)}>Admin</button>
        <p className="eyebrow">Published pages</p>
        {treeError ? <p className="status error">{treeError}</p> : null}
        {!tree && !treeError ? <p className="status">Loading notes…</p> : null}
        {tree && notes.length === 0 ? <p className="status">No notes have been published yet.</p> : null}
        <nav aria-label="Published notes">
          {notes.map((candidate) => (
            <button
              className={candidate.id === selectedNote?.id ? 'note-link active' : 'note-link'}
              key={candidate.id}
              onClick={() => selectNote(candidate)}
            >
              {candidate.title}
            </button>
          ))}
        </nav>
      </aside>
      {adminOpen ? <AdminPanel onClose={() => setAdminOpen(false)} /> : <article className="note-page">
        {!selectedNote && tree ? <h1>Choose a note</h1> : null}
        {selectedNote ? <p className="eyebrow">{selectedNote.slug}</p> : null}
        {selectedNote ? <h1>{selectedNote.title}</h1> : null}
        {selectedNote && !note && !noteError ? <p className="status">Loading page…</p> : null}
        {noteError ? <p className="status error">{noteError}</p> : null}
        {note ? <section className="prose">{note.document.content?.map(renderBlock)}</section> : null}
      </article>}
    </main>
  )
}

export default App
