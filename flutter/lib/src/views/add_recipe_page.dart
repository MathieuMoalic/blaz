import 'package:flutter/material.dart';
import 'package:file_selector/file_selector.dart';
import 'package:path/path.dart' as p;
import 'dart:io' show File;
import 'dart:typed_data';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart';

class AddRecipePage extends StatefulWidget {
  const AddRecipePage({super.key});
  @override
  State<AddRecipePage> createState() => _AddRecipePageState();
}

enum AddRecipeMode { choice, importUrl, importImage, manual }

class _AddRecipePageState extends State<AddRecipePage> {
  final _form = GlobalKey<FormState>();

  final _title = TextEditingController();
  final _source = TextEditingController();
  final _yieldText = TextEditingController();
  final _notes = TextEditingController();
  final _ingredientsRaw = TextEditingController();
  final _instructionsRaw = TextEditingController();

  final _importUrl = TextEditingController();
  bool _importing = false;
  int _importStep = 0; // 0=not started, 1-5=progress steps

  // image import state (up to 3 photos)
  final List<(String, Uint8List)> _importImages = []; // (filename, bytes)
  bool _importingImages = false;
  int _importImageStep = 0; // 0=not started, 1-4=progress steps

  XFile? _picked; // selected file (manual entry cover image)
  Uint8List? _preview; // preview bytes (web or when Android only returns URI)
  bool _busy = false;

  AddRecipeMode _mode = AddRecipeMode.choice;

  @override
  void dispose() {
    _title.dispose();
    _source.dispose();
    _yieldText.dispose();
    _notes.dispose();
    _ingredientsRaw.dispose();
    _instructionsRaw.dispose();
    super.dispose();
  }

  Future<void> _pickImage() async {
    final group = const XTypeGroup(
      label: 'images',
      extensions: ['jpg', 'jpeg', 'png', 'webp', 'gif'],
    );

    final x = await openFile(acceptedTypeGroups: [group]);
    if (x == null) return;

    // Always read bytes for consistent uploads + preview
    final bytes = await x.readAsBytes();

    setState(() {
      _picked = x;
      _preview = bytes;
    });
  }

  Future<void> _importFromUrl() async {
    final url = _importUrl.text.trim();
    if (url.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Please paste a recipe URL')),
      );
      return;
    }
    setState(() {
      _importing = true;
      _importStep = 1;
    });

    // Animate through steps while waiting for the API
    _animateImportSteps();

    try {
      // Get user's selected model
      final prefs = await SharedPreferences.getInstance();
      final model = prefs.getString('llm_model') ?? 'anthropic/claude-3.5-sonnet';

      final created = await importRecipeFromUrl(url: url, model: model);
      if (!mounted) return;
      setState(() => _importStep = 5); // Complete
      await Future.delayed(const Duration(milliseconds: 300));
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Imported: ${created.title}')));
      Navigator.pop(context, true); // trigger refresh in caller
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Import failed: $e')));
    } finally {
      if (mounted) setState(() {
        _importing = false;
        _importStep = 0;
      });
    }
  }

  void _animateImportSteps() async {
    // Animate through steps with realistic timing
    // Step 1: Fetching page (already set)
    await Future.delayed(const Duration(milliseconds: 1500));
    if (!mounted || !_importing) return;
    setState(() => _importStep = 2); // Extracting recipe

    await Future.delayed(const Duration(milliseconds: 3000));
    if (!mounted || !_importing) return;
    setState(() => _importStep = 3); // Structuring ingredients

    await Future.delayed(const Duration(milliseconds: 3000));
    if (!mounted || !_importing) return;
    setState(() => _importStep = 4); // Converting units

    // Step 5 (Saving) will be set when the API completes
  }

  Future<void> _submit() async {
    if (!_form.currentState!.validate()) return;

    final title = _title.text.trim();
    final source = _source.text.trim();
    final yieldText = _yieldText.text.trim();
    final notes = _notes.text.trim();
    final ingredients = splitLines(_ingredientsRaw.text)
        .map((line) => Ingredient(name: line.trim(), raw: true))
        .toList();
    final instructions = splitLines(_instructionsRaw.text);

    setState(() => _busy = true);
    try {
      final created = await createRecipeFull(
        title: title,
        source: source,
        yieldText: yieldText,
        notes: notes,
        ingredients: ingredients,
        instructions: instructions,
      );

      if (_picked != null) {
        final name = _picked!.name.isNotEmpty
            ? _picked!.name
            : (_picked!.path.isNotEmpty ? p.basename(_picked!.path) : 'upload');

        if (_preview != null) {
          await uploadRecipeImage(
            id: created.id,
            filename: name,
            bytes: _preview!, // <-- use bytes on Android/iOS too
          );
        } else if (_picked!.path.isNotEmpty) {
          await uploadRecipeImage(
            id: created.id,
            path: _picked!.path, // fallback if for some reason bytes missing
            filename: name,
          );
        } else {
          // Extremely rare: neither bytes nor path — show a friendly error
          throw Exception('No image data available from the picker');
        }
      }

      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Widget _buildChoiceScreen() {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'Add a Recipe',
              style: Theme.of(context).textTheme.headlineMedium,
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 32),
            FilledButton.icon(
              onPressed: () => setState(() => _mode = AddRecipeMode.importUrl),
              icon: const Icon(Icons.link),
              label: const Text('Import from URL'),
              style: FilledButton.styleFrom(
                padding: const EdgeInsets.all(20),
              ),
            ),
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: () =>
                  setState(() => _mode = AddRecipeMode.importImage),
              icon: const Icon(Icons.photo_camera),
              label: const Text('Import from Image'),
              style: FilledButton.styleFrom(
                padding: const EdgeInsets.all(20),
              ),
            ),
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: () => setState(() => _mode = AddRecipeMode.manual),
              icon: const Icon(Icons.edit),
              label: const Text('Enter Manually'),
              style: FilledButton.styleFrom(
                padding: const EdgeInsets.all(20),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildImportUrlScreen() {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Import from URL',
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                const SizedBox(height: 16),
                TextField(
                  controller: _importUrl,
                  decoration: const InputDecoration(
                    labelText: 'Recipe URL',
                    hintText: 'https://example.com/some-recipe',
                    border: OutlineInputBorder(),
                    prefixIcon: Icon(Icons.link),
                  ),
                  keyboardType: TextInputType.url,
                  textInputAction: TextInputAction.done,
                  autofillHints: const <String>[],
                  enabled: !_importing,
                  onSubmitted: (_) => _importFromUrl(),
                ),
                const SizedBox(height: 16),
                FilledButton.icon(
                  onPressed: _importing ? null : _importFromUrl,
                  icon: _importing
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                          ),
                        )
                      : const Icon(Icons.download),
                  label: const Text('Import'),
                  style: FilledButton.styleFrom(
                    minimumSize: const Size.fromHeight(48),
                  ),
                ),
              ],
            ),
          ),
        ),
        if (_importing) ...[
          const SizedBox(height: 16),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final (step, label) in [
                    (1, 'Fetching page...'),
                    (2, 'Extracting recipe...'),
                    (3, 'Structuring ingredients...'),
                    (4, 'Converting to metric...'),
                    (5, 'Saving recipe...'),
                  ])
                    _ImportStepTile(
                      label: label,
                      isActive: _importStep == step,
                      isComplete: _importStep > step,
                    ),
                ],
              ),
            ),
          ),
        ],
      ],
    );
  }

  Future<void> _addImportImage() async {
    if (_importImages.length >= 3) return;
    final group = const XTypeGroup(
      label: 'images',
      extensions: ['jpg', 'jpeg', 'png', 'webp', 'heic'],
    );
    final file = await openFile(acceptedTypeGroups: [group]);
    if (file == null) return;
    final bytes = await file.readAsBytes();
    setState(() => _importImages.add((file.name, bytes)));
  }

  Future<void> _submitImportImages() async {
    if (_importImages.isEmpty) return;
    setState(() {
      _importingImages = true;
      _importImageStep = 1;
    });

    // Animate through steps while waiting for the API
    _animateImageImportSteps();

    try {
      final created = await importRecipeFromImages(
        _importImages
            .map((e) => (e.$1, e.$2.toList()))
            .toList(),
      );
      if (!mounted) return;
      setState(() => _importImageStep = 4); // Complete
      await Future.delayed(const Duration(milliseconds: 300));
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Imported: ${created.title}')),
      );
      Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Import failed: $e')));
    } finally {
      if (mounted) setState(() {
        _importingImages = false;
        _importImageStep = 0;
      });
    }
  }

  void _animateImageImportSteps() async {
    // Step 1: Uploading images (already set)
    await Future.delayed(const Duration(milliseconds: 2000));
    if (!mounted || !_importingImages) return;
    setState(() => _importImageStep = 2); // Analyzing images

    await Future.delayed(const Duration(milliseconds: 4000));
    if (!mounted || !_importingImages) return;
    setState(() => _importImageStep = 3); // Extracting recipe

    // Step 4 (Saving) will be set when the API completes
  }

  Widget _buildImportImageScreen() {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Import from Photos',
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                const SizedBox(height: 4),
                Text(
                  'Add up to 3 photos — useful when the recipe spans multiple pages.',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                const SizedBox(height: 16),

                // Thumbnails
                if (_importImages.isNotEmpty) ...[
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      for (var i = 0; i < _importImages.length; i++)
                        Stack(
                          children: [
                            ClipRRect(
                              borderRadius: BorderRadius.circular(8),
                              child: Image.memory(
                                _importImages[i].$2,
                                width: 96,
                                height: 96,
                                fit: BoxFit.cover,
                              ),
                            ),
                            Positioned(
                              top: 2,
                              right: 2,
                              child: GestureDetector(
                                onTap: () =>
                                    setState(() => _importImages.removeAt(i)),
                                child: Container(
                                  decoration: const BoxDecoration(
                                    color: Colors.black54,
                                    shape: BoxShape.circle,
                                  ),
                                  padding: const EdgeInsets.all(2),
                                  child: const Icon(
                                    Icons.close,
                                    size: 14,
                                    color: Colors.white,
                                  ),
                                ),
                              ),
                            ),
                          ],
                        ),
                    ],
                  ),
                  const SizedBox(height: 12),
                ],

                Row(
                  children: [
                    if (_importImages.length < 3)
                      OutlinedButton.icon(
                        onPressed:
                            _importingImages ? null : _addImportImage,
                        icon: const Icon(Icons.add_photo_alternate_outlined),
                        label: Text(
                          _importImages.isEmpty
                              ? 'Add photo'
                              : 'Add another',
                        ),
                      ),
                    const Spacer(),
                    FilledButton.icon(
                      onPressed: (_importImages.isEmpty || _importingImages)
                          ? null
                          : _submitImportImages,
                      icon: _importingImages
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(
                                strokeWidth: 2,
                              ),
                            )
                          : const Icon(Icons.download),
                      label: const Text('Import'),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
        if (_importingImages) ...[
          const SizedBox(height: 16),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final (step, label) in [
                    (1, 'Uploading images...'),
                    (2, 'Analyzing images...'),
                    (3, 'Extracting recipe...'),
                    (4, 'Saving recipe...'),
                  ])
                    _ImportStepTile(
                      label: label,
                      isActive: _importImageStep == step,
                      isComplete: _importImageStep > step,
                    ),
                ],
              ),
            ),
          ),
        ],
      ],
    );
  }

  Widget _buildManualScreen() {
    final gap = const SizedBox(height: 12);

    final selectedName = _picked == null
        ? 'No image selected'
        : (_picked!.name.isNotEmpty
              ? _picked!.name
              : (_picked!.path.isNotEmpty
                    ? p.basename(_picked!.path)
                    : 'Selected image'));

    final hasBytesPreview = _preview != null;
    final hasFilePreview =
        !kIsWeb && _picked != null && _picked!.path.isNotEmpty;

    return Form(
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
            autofillHints: const <String>[],
            validator: (v) =>
                (v == null || v.trim().isEmpty) ? 'Title required' : null,
          ),
          gap,

          // Image
          Row(
            children: [
              FilledButton.icon(
                onPressed: _busy ? null : _pickImage,
                icon: const Icon(Icons.photo),
                label: const Text('Choose image'),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(selectedName, overflow: TextOverflow.ellipsis),
              ),
            ],
          ),
          if (hasBytesPreview || hasFilePreview) ...[
            const SizedBox(height: 8),
            SizedBox(
              height: 120,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(8),
                child: hasBytesPreview
                    ? Image.memory(_preview!, fit: BoxFit.cover)
                    : Image.file(File(_picked!.path), fit: BoxFit.cover),
              ),
            ),
          ],
          gap,

          // Ingredients
          TextField(
            controller: _ingredientsRaw,
            decoration: const InputDecoration(
              labelText: 'Ingredients (one per line)',
              hintText: 'e.g.\n2 cloves garlic\n150 g flour\nPinch of salt',
              border: OutlineInputBorder(),
              alignLabelWithHint: true,
            ),
            maxLines: 6,
            autofillHints: const <String>[],
          ),
          gap,

          // Instructions
          TextField(
            controller: _instructionsRaw,
            decoration: const InputDecoration(
              labelText: 'Instructions (one step per line)',
              hintText:
                  'e.g.\nMince the garlic.\nFold in flour.\nBake 20 min.',
              border: OutlineInputBorder(),
              alignLabelWithHint: true,
            ),
            maxLines: 8,
            autofillHints: const <String>[],
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
            maxLines: 4,
            autofillHints: const <String>[],
          ),
          gap,

          // Source
          TextField(
            controller: _source,
            decoration: const InputDecoration(
              labelText: 'Source (URL, book, person…)',
              border: OutlineInputBorder(),
            ),
            textInputAction: TextInputAction.next,
            autofillHints: const <String>[],
          ),
          gap,

          // Yield
          TextField(
            controller: _yieldText,
            decoration: const InputDecoration(
              labelText: 'Yield (e.g. "4 servings")',
              border: OutlineInputBorder(),
            ),
            textInputAction: TextInputAction.next,
            autofillHints: const <String>[],
          ),

          const SizedBox(height: 16),
          FilledButton.icon(
            onPressed: _busy ? null : _submit,
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.check),
            label: const Text('Create'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Add recipe'),
        leading: _mode != AddRecipeMode.choice
            ? IconButton(
                icon: const Icon(Icons.arrow_back),
                onPressed: () => setState(() => _mode = AddRecipeMode.choice),
              )
            : null,
      ),
      body: SafeArea(
        child: switch (_mode) {
          AddRecipeMode.choice => _buildChoiceScreen(),
          AddRecipeMode.importUrl => _buildImportUrlScreen(),
          AddRecipeMode.importImage => _buildImportImageScreen(),
          AddRecipeMode.manual => _buildManualScreen(),
        },
      ),
    );
  }
}

class _ImportStepTile extends StatelessWidget {
  final String label;
  final bool isActive;
  final bool isComplete;

  const _ImportStepTile({
    required this.label,
    required this.isActive,
    required this.isComplete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          SizedBox(
            width: 24,
            height: 24,
            child: isComplete
                ? Icon(Icons.check_circle, color: theme.colorScheme.primary, size: 20)
                : isActive
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : Icon(Icons.circle_outlined, color: theme.disabledColor, size: 18),
          ),
          const SizedBox(width: 12),
          Text(
            label,
            style: TextStyle(
              color: isComplete || isActive ? null : theme.disabledColor,
              fontWeight: isActive ? FontWeight.bold : null,
            ),
          ),
        ],
      ),
    );
  }
}
