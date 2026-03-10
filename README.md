
## Stock symbol examples
```
MSFT
BHP.AX
```

## Run
```
python -m venv venv

source venv/bin/activate

pip install -r requirements.txt

python3 app.py
```

## Build and Run Docker
```
docker build -t stock-tracker .

docker run --rm -p 5000:5000 -v stock_data:/app/data stock-tracker

docker run -d -p 5000:5000 -v stock_data:/app/data stock-tracker

docker volume inspect stock_data
sudo ls -la /var/lib/docker/volumes/stock_data/_data
```
