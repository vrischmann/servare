default: dev

dev:
	sqlx database setup
	cargo sqlx prepare -- --all-targets --all-features
	cargo watch -x run

install-tools:
	cargo install sqlx-cli --no-default-features --features rustls,sqlite,postgres
	cargo install cargo-watch cargo-deb grcov

run-jaeger:
	docker run -d --name jaeger \
		-e COLLECTOR_ZIPKIN_HOST_PORT=:9411 \
		-e COLLECTOR_OTLP_ENABLED=true \
		-p 6831:6831/udp \
		-p 6832:6832/udp \
		-p 5778:5778 \
		-p 16686:16686 \
		-p 4317:4317 \
		-p 4318:4318 \
		-p 14250:14250 \
		-p 14268:14268 \
		-p 14269:14269 \
		-p 9411:9411 \
		jaegertracing/all-in-one:1.41

test:
	cargo test

cover:
	RUSTFLAGS="-Cinstrument-coverage" cargo build
	RUSTFLAGS="-Cinstrument-coverage" LLVM_PROFILE_FILE="servare-%p-%m.profraw" cargo test
	grcov . -s . --binary-path ./target/debug/ -t html --branch --ignore-not-existing -o ./target/debug/coverage/
