server:
	cargo run --

test_client:
	socat stdio /tmp/test.sock <<<'{"method":"ping","params":null}'

build:
	cargo build

# vim:ft=make:
