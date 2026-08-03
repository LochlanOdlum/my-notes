import { useEffect, useMemo, useState } from 'react'

const publishedBaseUrl = (
  import.meta.env.VITE_PUBLISHED_CONTENT_URL ??
  'https://d81ul6xa7pt91.cloudfront.net'
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
  status: 'published'
  publishedRevision: string
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

function textContent(node: DocumentNode): string {
  if (node.type === 'text') return node.text ?? ''
  return node.content?.map(textContent).join('') ?? ''
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

function App() {
  const [tree, setTree] = useState<TreeManifest | null>(null)
  const [treeError, setTreeError] = useState<string | null>(null)
  const [selectedSlug, setSelectedSlug] = useState(noteSlugFromHash)
  const [note, setNote] = useState<PublishedNote | null>(null)
  const [noteError, setNoteError] = useState<string | null>(null)

  useEffect(() => {
    const controller = new AbortController()
    fetch(`${publishedBaseUrl}/tree.json`, { signal: controller.signal })
      .then(async (response) => {
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
      (tree?.nodes.filter((node): node is NoteNode => node.type === 'note') ?? []).sort(
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
      <article className="note-page">
        {!selectedNote && tree ? <h1>Choose a note</h1> : null}
        {selectedNote ? <p className="eyebrow">{selectedNote.slug}</p> : null}
        {selectedNote ? <h1>{selectedNote.title}</h1> : null}
        {selectedNote && !note && !noteError ? <p className="status">Loading page…</p> : null}
        {noteError ? <p className="status error">{noteError}</p> : null}
        {note ? <section className="prose">{note.document.content?.map(renderBlock)}</section> : null}
      </article>
    </main>
  )
}

export default App
