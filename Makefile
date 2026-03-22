.PHONY: build test lint serve migrate swagger clean

build:
	go build -o bin/kassi ./cmd/kassi

test:
	go test -race ./...

lint:
	golangci-lint run ./...

serve:
	go run ./cmd/kassi serve

migrate:
	go run ./cmd/kassi migrate

swagger:
	swag init -g cmd/kassi/main.go -o internal/docs

clean:
	rm -rf bin/
