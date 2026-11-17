#!/usr/bin/env ruby
# No key, network, workflow execution or provider process; policy inputs only.
require 'fileutils'
require 'open3'
require 'tmpdir'
require 'yaml'

policy = File.expand_path(ARGV.fetch(0))
source = File.expand_path('../../..', __dir__)
workflow = '.github/workflows/wave3-openrouter-live.yml'
scripts = %w[
  .github/scripts/runtime-live-preflight/openrouter.py
  .github/scripts/runtime-live-preflight/test_openrouter.py
  .github/scripts/wave3-qualification/openrouter_job.py
  .github/scripts/wave3-qualification/test_openrouter_job.py
]
original = File.read(File.join(source, workflow))

def check(policy, name, expected)
  Dir.mktmpdir('receipts-openrouter-policy-') do |root|
    root = File.realpath(root)
    directory = File.join(root, '.github/workflows')
    FileUtils.mkdir_p(directory)
    yield root, directory
    output, status = Open3.capture2e('ruby', policy, directory)
    raise "#{name}: unexpected policy result" unless status.success? == expected
    raise "#{name}: missing secret refusal" unless expected || output.include?('secret exposed to job')
    puts "PASS #{name}"
  end
end

seed = lambda do |root|
  ([workflow] + scripts).each do |relative|
    target = File.join(root, relative)
    FileUtils.mkdir_p(File.dirname(target))
    FileUtils.cp(File.join(source, relative), target)
  end
end
check(policy, 'exact reviewed workflow plus executable pins', true) { |root, _| seed.call(root) }

mutations = {
  'push trigger' => ->(d) { d[d.key?('on') ? 'on' : true] = 'push' },
  'missing repository guard' => ->(d) { d['jobs']['openrouter']['if'] = "github.run_attempt == 1" },
  'different ref guard' => ->(d) { d['jobs']['openrouter']['if'].sub!('codex/wave3-live-qualification', 'unreviewed') },
  'rerun allowed' => ->(d) { d['jobs']['openrouter']['if'].sub!('github.run_attempt == 1', 'true') },
  'wrong environment' => ->(d) { d['jobs']['openrouter']['environment'] = 'unprotected' },
  'write permissions' => ->(d) { d['permissions']['contents'] = 'write' },
  'self-hosted runner' => ->(d) { d['jobs']['openrouter']['runs-on'] = 'self-hosted' },
  'unbounded timeout' => ->(d) { d['jobs']['openrouter'].delete('timeout-minutes') },
  'secret at job scope' => ->(d) { d['jobs']['openrouter']['env'] = {'KEY' => '${{ secrets.OPENROUTER_API_KEY }}'} },
  'different secret' => ->(d) { d['jobs']['openrouter']['steps'][2]['env']['OPENROUTER_API_KEY'] = '${{ secrets.OTHER_KEY }}' },
  'second secret' => ->(d) { d['jobs']['openrouter']['steps'][2]['env']['OTHER'] = '${{ secrets.OTHER_KEY }}' },
  'arbitrary secret command' => ->(d) { d['jobs']['openrouter']['steps'][2]['run'] = 'echo "$OPENROUTER_API_KEY"' },
  'persisted checkout token' => ->(d) { d['jobs']['openrouter']['steps'][0]['with']['persist-credentials'] = true },
  'unselected checkout' => ->(d) { d['jobs']['openrouter']['steps'][0]['with']['ref'] = 'main' },
  'broad artifact upload' => ->(d) { d['jobs']['openrouter']['steps'][-1]['with']['path'] = '${{ runner.temp }}' },
  'additional job' => ->(d) { d['jobs']['other'] = {'runs-on' => 'ubuntu-24.04', 'steps' => [{'run' => 'echo "${{ secrets.OTHER }}"'}]} },
  'reusable secret inheritance' => ->(d) { d['jobs']['other'] = {'uses' => './other.yml', 'secrets' => 'inherit'} }
}
mutations.each do |name, mutate|
  check(policy, name, false) do |root, _|
    seed.call(root)
    doc = YAML.safe_load(original)
    mutate.call(doc)
    File.write(File.join(root, workflow), YAML.dump(doc))
  end
end
check(policy, 'identical bytes under different workflow filename', false) do |root, directory|
  seed.call(root)
  FileUtils.mv(File.join(root, workflow), File.join(directory, 'renamed.yml'))
end
scripts.each do |script|
  check(policy, "changed executed source #{File.basename(script)}", false) do |root, _|
    seed.call(root)
    File.open(File.join(root, script), 'a') { |f| f.puts '# changed' }
  end
end
check(policy, 'missing executed source', false) do |root, _|
  seed.call(root)
  FileUtils.rm(File.join(root, scripts.first))
end
check(policy, 'symlinked executed source', false) do |root, _|
  seed.call(root)
  path = File.join(root, scripts.first)
  FileUtils.mv(path, path + '.copy')
  File.symlink(path + '.copy', path)
end
# Identical reviewed leaves must still refuse when any ancestor redirects.
%w[.github .github/scripts .github/workflows .github/scripts/runtime-live-preflight .github/scripts/wave3-qualification].each do |relative|
  check(policy, "redirected ancestor #{relative}", false) do |root, _|
    seed.call(root)
    path = File.join(root, relative)
    moved = File.join(root, 'redirected')
    FileUtils.mv(path, moved)
    File.symlink(moved, path)
  end
end
check(policy, 'general secret rule still rejects unrelated workflow', false) do |_root, directory|
  File.write(File.join(directory, 'other.yml'), "on: workflow_dispatch\npermissions: {contents: read}\njobs: {x: {runs-on: ubuntu-24.04, steps: [{run: 'echo ${{ secrets.ANY_KEY }}'}]}}\n")
end
check(policy, 'general credential-free workflow remains allowed', true) do |_root, directory|
  File.write(File.join(directory, 'safe.yml'), "on: workflow_dispatch\npermissions: {contents: read}\njobs: {x: {runs-on: ubuntu-24.04, steps: [{run: 'echo safe'}]}}\n")
end
