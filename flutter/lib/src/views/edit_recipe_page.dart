import 'package:flutter/material.dart';
import 'package:file_selector/file_selector.dart';
import '../api.dart';

class EditRecipePage extends StatefulWidget {
  final Recipe recipe;
  const EditRecipePage({super.key, required this.recipe});

  @override
  State<EditRecipePage> createState() => _EditRecipePageState();
}

class _EditRecipePageState extends State<EditRecipePage> {
  final _form = GlobalKey<FormState>();

  late final TextEditingController _title;
  late final TextEditingController _source;
  late final TextEditingController _yieldText;
  late final TextEditingController _notes;
  late final TextEditingController _instructionsRaw;

  late List<Ingredient> _ingredients;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    final r = widget.recipe;
    _title = TextEditingController(text: r.title);
    _source = TextEditingController(text: r.source);
    _yieldText = TextEditingController(text: r.yieldText);
    _notes = TextEditingController(text: r.notes);
    _instructionsRaw = TextEditingController(text: r.instructions.join('\n'));
    _ingredients = List.from(r.ingredients);
  }

  @override
  void dispose() {
    _title.dispose();
    _source.dispose();
    _yieldText.dispose();
    _notes.dispose();
    _instructionsRaw.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (!_form.currentState!.validate()) return;
    setState(() => _busy = true);
    try {
      await updateRecipe(
        id: widget.recipe.id,
        title: _title.text.trim(),
        source: _source.text.trim(),
        yieldText: _yieldText.text.trim(),
        notes: _notes.text.trim(),
        ingredients: _ingredients,
        instructions: splitLines(_instructionsRaw.text),
      );
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _changeImage() async {
    final typeGroup = const XTypeGroup(
      label: 'images',
      extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
    );
    final file = await openFile(acceptedTypeGroups: [typeGroup]);
    if (file == null) return;

    setState(() => _busy = true);
    try {
      final bytes = await file.readAsBytes();
      await uploadRecipeImage(id: widget.recipe.id, filename: file.name, bytes: bytes);
      if (!mounted) return;
      Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Image failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  /// Opens an edit dialog for the ingredient at [index], or -1 to add a new one.
  Future<void> _editIngredient(int index) async {
    final existing = index >= 0 ? _ingredients[index] : null;

    final result = await showDialog<Ingredient>(
      context: context,
      builder: (ctx) => _IngredientDialog(
        title: index >= 0 ? 'Edit ingredient' : 'Add ingredient',
        initial: existing,
      ),
    );

    if (result != null) {
      setState(() {
        if (index >= 0) {
          _ingredients[index] = result;
        } else {
          _ingredients.add(result);
        }
      });
    }
  }

  Future<void> _reparseWithAi() async {
    setState(() => _busy = true);
    try {
      final parsed = await reparseIngredients(widget.recipe.id);
      if (!mounted) return;
      // Merge: keep any ingredients the LLM didn't return (by index)
      setState(() {
        for (var i = 0; i < parsed.length && i < _ingredients.length; i++) {
          _ingredients[i] = parsed[i];
        }
      });
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('AI re-parse failed: $e')),
      );
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _parseAll() async {
    final rawIndices = <int>[];
    for (var i = 0; i < _ingredients.length; i++) {
      if (_ingredients[i].raw) rawIndices.add(i);
    }
    if (rawIndices.isEmpty) return;

    List<String> knownNames;
    try {
      knownNames = await fetchAllShoppingTexts();
    } catch (_) {
      knownNames = const [];
    }

    for (final i in rawIndices) {
      if (!mounted) return;
      final result = await showDialog<Ingredient>(
        context: context,
        barrierDismissible: false,
        builder: (ctx) => ParseIngredientDialog(
          rawText: _ingredients[i].name,
          knownNames: knownNames,
          current: rawIndices.indexOf(i) + 1,
          total: rawIndices.length,
        ),
      );
      if (result != null) {
        setState(() => _ingredients[i] = result);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final gap = const SizedBox(height: 12);
    final theme = Theme.of(context);
    final hasUnparsed = _ingredients.any((i) => i.raw);

    return Scaffold(
      appBar: AppBar(title: const Text('Edit recipe')),
      body: SafeArea(
        child: Form(
          key: _form,
          child: ListView(
            padding: const EdgeInsets.all(16),
            children: [
              // Title
              TextFormField(
                controller: _title,
                decoration: const InputDecoration(
                  labelText: 'Title *',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
                validator: (v) =>
                    (v == null || v.trim().isEmpty) ? 'Title required' : null,
              ),
              gap,

              // Image
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      const Icon(Icons.photo),
                      const SizedBox(width: 12),
                      const Expanded(child: Text('Recipe Image')),
                      FilledButton.icon(
                        onPressed: _busy ? null : _changeImage,
                        icon: const Icon(Icons.photo_outlined),
                        label: const Text('Change image'),
                      ),
                    ],
                  ),
                ),
              ),
              gap,

              // Ingredients
              Row(
                children: [
                  Text('Ingredients', style: theme.textTheme.titleSmall),
                  const Spacer(),
                  IconButton(
                    icon: const Icon(Icons.auto_awesome, size: 18),
                    tooltip: 'Re-parse with AI',
                    onPressed: _busy ? null : _reparseWithAi,
                    visualDensity: VisualDensity.compact,
                  ),
                  if (hasUnparsed)
                    TextButton.icon(
                      icon: const Icon(Icons.auto_fix_high, size: 16),
                      label: const Text('Parse all'),
                      onPressed: _busy ? null : _parseAll,
                    ),
                ],
              ),
              const SizedBox(height: 4),
              Card(
                child: Column(
                  children: [
                    for (int i = 0; i < _ingredients.length; i++)
                      _IngredientTile(
                        ingredient: _ingredients[i],
                        onTap: _busy ? null : () => _editIngredient(i),
                        onDelete: _busy
                            ? null
                            : () => setState(() => _ingredients.removeAt(i)),
                      ),
                    ListTile(
                      leading: const Icon(Icons.add),
                      title: const Text('Add ingredient'),
                      onTap: _busy ? null : () => _editIngredient(-1),
                    ),
                  ],
                ),
              ),
              gap,

              // Instructions
              TextField(
                controller: _instructionsRaw,
                decoration: const InputDecoration(
                  labelText: 'Instructions (one step per line)',
                  hintText: 'e.g.\nFold in flour.\nBake 20 min.',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                minLines: 5,
                maxLines: null,
              ),
              gap,

              // Notes
              TextField(
                controller: _notes,
                decoration: const InputDecoration(
                  labelText: 'Notes',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                minLines: 2,
                maxLines: null,
              ),
              gap,

              // Source
              TextField(
                controller: _source,
                decoration: const InputDecoration(
                  labelText: 'Source',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
              ),
              gap,

              // Yield
              TextField(
                controller: _yieldText,
                decoration: const InputDecoration(
                  labelText: 'Yield',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: _busy ? null : _save,
                icon: _busy
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.check),
                label: const Text('Save'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Helpers

String _fmtQty(double v) {
  final s = ((v * 100).round() / 100.0).toString();
  return s.endsWith('.0') ? s.replaceFirst('.0', '') : s;
}

// ---------------------------------------------------------------------------
// Ingredient tile

class _IngredientTile extends StatelessWidget {
  final Ingredient ingredient;
  final VoidCallback? onTap;
  final VoidCallback? onDelete;

  const _IngredientTile({required this.ingredient, this.onTap, this.onDelete});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    final isRaw = ingredient.raw;

    final qtyLabel = isRaw
        ? '?'
        : ingredient.quantity != null
            ? [
                _fmtQty(ingredient.quantity!),
                if (ingredient.unit != null) ingredient.unit!,
              ].join('\u00a0') // non-breaking space
            : '—';

    return ListTile(
      dense: true,
      leading: SizedBox(
        width: 52,
        child: Text(
          qtyLabel,
          style: theme.textTheme.bodySmall?.copyWith(
            color: muted,
            fontFeatures: const [FontFeature.tabularFigures()],
          ),
          textAlign: TextAlign.end,
        ),
      ),
      title: Text(
        ingredient.name,
        style: isRaw ? TextStyle(color: muted) : null,
      ),
      subtitle: (ingredient.prep != null && ingredient.prep!.isNotEmpty)
          ? Text(ingredient.prep!,
              style: theme.textTheme.bodySmall?.copyWith(color: muted))
          : null,
      trailing: IconButton(
        icon: const Icon(Icons.close, size: 18),
        onPressed: onDelete,
        visualDensity: VisualDensity.compact,
      ),
      onTap: onTap,
    );
  }
}

// ---------------------------------------------------------------------------
// Ingredient edit dialog

class _IngredientDialog extends StatefulWidget {
  final String title;
  final Ingredient? initial;

  const _IngredientDialog({required this.title, this.initial});

  @override
  State<_IngredientDialog> createState() => _IngredientDialogState();
}

class _IngredientDialogState extends State<_IngredientDialog> {
  late final TextEditingController _qty;
  late final TextEditingController _unit;
  late final TextEditingController _name;
  late final TextEditingController _prep;

  @override
  void initState() {
    super.initState();
    final i = widget.initial;
    _qty  = TextEditingController(text: i?.quantity != null ? _fmtQty(i!.quantity!) : '');
    _unit = TextEditingController(text: i?.unit ?? '');
    _name = TextEditingController(text: i?.name ?? '');
    _prep = TextEditingController(text: i?.prep ?? '');
  }

  @override
  void dispose() {
    _qty.dispose();
    _unit.dispose();
    _name.dispose();
    _prep.dispose();
    super.dispose();
  }

  void _submit() {
    final name = _name.text.trim();
    if (name.isEmpty) return;
    Navigator.pop(
      context,
      Ingredient(
        quantity: double.tryParse(_qty.text.trim().replaceAll(',', '.')),
        unit: _unit.text.trim().isEmpty ? null : _unit.text.trim(),
        name: name,
        prep: _prep.text.trim().isEmpty ? null : _prep.text.trim(),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    const gap = SizedBox(height: 12);
    return AlertDialog(
      title: Text(widget.title),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              children: [
                Flexible(
                  flex: 2,
                  child: TextField(
                    controller: _qty,
                    decoration: const InputDecoration(
                      labelText: 'Qty',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                    keyboardType: const TextInputType.numberWithOptions(decimal: true),
                  ),
                ),
                const SizedBox(width: 8),
                Flexible(
                  flex: 2,
                  child: TextField(
                    controller: _unit,
                    decoration: const InputDecoration(
                      labelText: 'Unit',
                      hintText: 'g, ml…',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
              ],
            ),
            gap,
            TextField(
              controller: _name,
              autofocus: true,
              decoration: const InputDecoration(
                labelText: 'Name *',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _submit(),
            ),
            gap,
            TextField(
              controller: _prep,
              decoration: const InputDecoration(
                labelText: 'Prep (optional)',
                hintText: 'diced, sifted…',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _submit(),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: _submit,
          child: const Text('Save'),
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Parse-all confirmation dialog

// ---------------------------------------------------------------------------
// Parse-one-at-a-time dialog

String _normalizeName(String s) {
  final lower = s.toLowerCase().trim();
  if (lower.endsWith('ies')) return '${lower.substring(0, lower.length - 3)}y';
  if (lower.endsWith('ves')) return '${lower.substring(0, lower.length - 3)}f';
  if (lower.endsWith('es') && lower.length > 3) {
    return lower.substring(0, lower.length - 2);
  }
  if (lower.endsWith('s') && lower.length > 2) {
    return lower.substring(0, lower.length - 1);
  }
  return lower;
}

class ParseIngredientDialog extends StatefulWidget {
  final String rawText;
  final List<String> knownNames;
  final int current;
  final int total;

  const ParseIngredientDialog({
    super.key,
    required this.rawText,
    required this.knownNames,
    required this.current,
    required this.total,
  });

  @override
  State<ParseIngredientDialog> createState() => _ParseIngredientDialogState();
}

class _ParseIngredientDialogState extends State<ParseIngredientDialog> {
  late final TextEditingController _qty;
  late final TextEditingController _unit;
  late final TextEditingController _name;
  late final TextEditingController _prep;

  @override
  void initState() {
    super.initState();
    final p = parseIngredientLine(widget.rawText);
    _qty  = TextEditingController(text: p.quantity != null ? _fmtQty(p.quantity!) : '');
    _unit = TextEditingController(text: p.unit ?? '');
    _name = TextEditingController(text: p.name);
    _prep = TextEditingController(text: p.prep ?? '');
    _name.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _qty.dispose();
    _unit.dispose();
    _name.dispose();
    _prep.dispose();
    super.dispose();
  }

  List<String> get _suggestions {
    final currentName = _name.text.trim();
    if (currentName.isEmpty) return const [];
    final norm = _normalizeName(currentName);
    return widget.knownNames
        .where((k) => _normalizeName(k) == norm && k != currentName)
        .take(5)
        .toList();
  }

  void _submit() {
    final name = _name.text.trim();
    if (name.isEmpty) return;
    Navigator.pop(
      context,
      Ingredient(
        quantity: double.tryParse(_qty.text.trim().replaceAll(',', '.')),
        unit: _unit.text.trim().isEmpty ? null : _unit.text.trim(),
        name: name,
        prep: _prep.text.trim().isEmpty ? null : _prep.text.trim(),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    final suggestions = _suggestions;
    const gap = SizedBox(height: 12);

    return AlertDialog(
      title: Row(
        children: [
          Expanded(child: Text('Parse ingredient ${widget.current}/${widget.total}')),
        ],
      ),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              decoration: BoxDecoration(
                color: theme.colorScheme.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(6),
              ),
              child: Text(
                widget.rawText,
                style: theme.textTheme.bodySmall?.copyWith(color: muted),
              ),
            ),
            gap,
            Row(
              children: [
                Flexible(
                  flex: 2,
                  child: TextField(
                    controller: _qty,
                    decoration: const InputDecoration(
                      labelText: 'Qty',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                    keyboardType:
                        const TextInputType.numberWithOptions(decimal: true),
                  ),
                ),
                const SizedBox(width: 8),
                Flexible(
                  flex: 2,
                  child: TextField(
                    controller: _unit,
                    decoration: const InputDecoration(
                      labelText: 'Unit',
                      hintText: 'g, ml…',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
              ],
            ),
            gap,
            TextField(
              controller: _name,
              autofocus: true,
              decoration: const InputDecoration(
                labelText: 'Name *',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _submit(),
            ),
            if (suggestions.isNotEmpty) ...[
              const SizedBox(height: 6),
              Wrap(
                spacing: 6,
                children: [
                  for (final s in suggestions)
                    ActionChip(
                      label: Text(s),
                      visualDensity: VisualDensity.compact,
                      onPressed: () => setState(() => _name.text = s),
                    ),
                ],
              ),
            ],
            gap,
            TextField(
              controller: _prep,
              decoration: const InputDecoration(
                labelText: 'Prep (optional)',
                hintText: 'diced, sifted…',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _submit(),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Skip'),
        ),
        FilledButton(
          onPressed: _name.text.trim().isEmpty ? null : _submit,
          child: const Text('Save'),
        ),
      ],
    );
  }
}
