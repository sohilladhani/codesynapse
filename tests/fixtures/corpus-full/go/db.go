package main

import (
	"fmt"
	"sync"
)

type Database struct {
	mu    sync.RWMutex
	store map[string]interface{}
}

func NewDatabase() *Database {
	return &Database{store: make(map[string]interface{})}
}

func (db *Database) Connect() error {
	return nil
}

func (db *Database) Disconnect() {}

func (db *Database) SaveUser(u *User) {
	db.mu.Lock()
	defer db.mu.Unlock()
	db.store[fmt.Sprintf("user:%d", u.ID)] = u
}

func (db *Database) FindUser(id int) (*User, bool) {
	db.mu.RLock()
	defer db.mu.RUnlock()
	v, ok := db.store[fmt.Sprintf("user:%d", id)]
	if !ok {
		return nil, false
	}
	return v.(*User), true
}

func (db *Database) SaveProduct(p *Product) {
	db.mu.Lock()
	defer db.mu.Unlock()
	db.store[fmt.Sprintf("product:%d", p.ID)] = p
}

func (db *Database) FindProduct(id int) (*Product, bool) {
	db.mu.RLock()
	defer db.mu.RUnlock()
	v, ok := db.store[fmt.Sprintf("product:%d", id)]
	if !ok {
		return nil, false
	}
	return v.(*Product), true
}

func (db *Database) SaveOrder(o *Order) {
	db.mu.Lock()
	defer db.mu.Unlock()
	db.store[fmt.Sprintf("order:%d", o.ID)] = o
}

func (db *Database) FindOrder(id int) (*Order, bool) {
	db.mu.RLock()
	defer db.mu.RUnlock()
	v, ok := db.store[fmt.Sprintf("order:%d", id)]
	if !ok {
		return nil, false
	}
	return v.(*Order), true
}
